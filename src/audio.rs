//! Own only audio processes started by this session. Reuse a user's existing stack.
use serde::Serialize;
use smithay::reexports::rustix::process::{kill_process, Pid, Signal};
use std::{
    ffi::OsString,
    os::unix::net::UnixStream,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
pub struct AudioStatus {
    pub enabled: bool,
    pub state: String,
    pub started: Vec<String>,
    pub reused: Vec<String>,
    pub error: Option<String>,
}
impl Default for AudioStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            state: "disabled".into(),
            started: vec![],
            reused: vec![],
            error: None,
        }
    }
}

pub fn control_args(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "volume-up" => Some(&[
            "set-volume",
            "--limit",
            "1.0",
            "@DEFAULT_AUDIO_SINK@",
            "5%+",
        ]),
        "volume-down" => Some(&["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"]),
        "mute" => Some(&["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]),
        "mic-mute" => Some(&["set-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"]),
        _ => None,
    }
}
#[derive(Clone)]
struct Context {
    runtime: PathBuf,
    env: Vec<(OsString, OsString)>,
}
impl Context {
    fn current() -> Self {
        Self {
            runtime: std::env::var_os("PIPEWIRE_RUNTIME_DIR")
                .or_else(|| std::env::var_os("XDG_RUNTIME_DIR"))
                .map(PathBuf::from)
                .unwrap_or_default(),
            env: vec![],
        }
    }
    fn command(&self, name: &str) -> Command {
        let mut command = Command::new(name);
        command.envs(self.env.iter().map(|(k, v)| (k, v)));
        command
    }
    fn socket(&self, name: &str) -> bool {
        UnixStream::connect(self.runtime.join(name)).is_ok()
    }
    fn manager_present(&self) -> bool {
        self.output("pw-cli", &["ls", "Client"]).is_ok_and(|o| {
            o.status.success()
                && String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l.trim() == "application.name = \"WirePlumber\"")
        })
    }
    fn output(&self, name: &str, args: &[&str]) -> Result<Output, String> {
        let mut child = self
            .command(name)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{name}: {e}"))?;
        let until = Instant::now() + Duration::from_secs(2);
        while child.try_wait().map_err(|e| e.to_string())?.is_none() {
            if Instant::now() >= until {
                stop(&mut child);
                return Err(format!("{name} probe timed out"));
            }
            thread::sleep(Duration::from_millis(20));
        }
        child.wait_with_output().map_err(|e| e.to_string())
    }
}
struct Service {
    name: String,
    process: Child,
}
pub struct AudioSession {
    services: Vec<Service>,
    pub status: AudioStatus,
}
impl AudioSession {
    pub fn start() -> Self {
        Self::start_in(Context::current())
    }
    fn start_in(context: Context) -> Self {
        let mut session = Self {
            services: vec![],
            status: AudioStatus {
                enabled: true,
                state: "starting".into(),
                ..Default::default()
            },
        };
        if let Err(error) = session.initialize(&context) {
            tracing::warn!(%error,"audio startup incomplete");
            session.status.error = Some(error);
            session.status.state = "degraded".into();
        } else {
            session.status.state = "running".into();
        }
        tracing::info!(started=?session.status.started,reused=?session.status.reused,state=%session.status.state,"audio session initialized");
        session
    }
    fn launch(&mut self, context: &Context, name: &str) -> Result<(), String> {
        let child = context
            .command(name)
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start {name}: {e}"))?;
        self.services.push(Service {
            name: name.into(),
            process: child,
        });
        self.status.started.push(name.into());
        Ok(())
    }
    fn wait_socket(&mut self, context: &Context, socket: &str) -> Result<(), String> {
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            self.poll();
            if let Some(error) = &self.status.error {
                return Err(error.clone());
            }
            if context.socket(socket) {
                return Ok(());
            }
            if Instant::now() >= until {
                return Err(format!("audio server did not open {socket}"));
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
    fn initialize(&mut self, context: &Context) -> Result<(), String> {
        if context.runtime.as_os_str().is_empty() {
            return Err("audio requires XDG_RUNTIME_DIR".into());
        }
        if context.socket("pipewire-0") {
            self.status.reused.push("pipewire".into());
        } else {
            self.launch(context, "pipewire")?;
            self.wait_socket(context, "pipewire-0")?;
        }
        if context.socket("pulse/native") {
            self.status.reused.push("pipewire-pulse".into());
        } else {
            self.launch(context, "pipewire-pulse")?;
            self.wait_socket(context, "pulse/native")?;
        }
        if context.manager_present() {
            self.status.reused.push("wireplumber".into());
        } else {
            self.launch(context, "wireplumber")?;
            let until = Instant::now() + Duration::from_secs(3);
            while !context.manager_present() {
                self.poll();
                if let Some(error) = &self.status.error {
                    return Err(error.clone());
                }
                if Instant::now() >= until {
                    return Err("WirePlumber did not register with PipeWire".into());
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
        Ok(())
    }
    pub fn poll(&mut self) {
        for service in &mut self.services {
            match service.process.try_wait() {
                Ok(Some(status)) => {
                    if self.status.error.is_none() {
                        let error = format!("{} exited ({status})", service.name);
                        tracing::warn!(%error,"audio service stopped");
                        self.status.error = Some(error);
                        self.status.state = "degraded".into();
                    }
                }
                Err(error) => {
                    self.status.error = Some(error.to_string());
                    self.status.state = "degraded".into();
                }
                _ => (),
            }
        }
    }
}
impl Drop for AudioSession {
    fn drop(&mut self) {
        for service in self.services.iter_mut().rev() {
            stop(&mut service.process);
        }
    }
}
fn stop(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    if let Some(pid) = Pid::from_raw(child.id() as i32) {
        let _ = kill_process(pid, Signal::TERM);
    }
    let until = Instant::now() + Duration::from_secs(2);
    while matches!(child.try_wait(), Ok(None)) && Instant::now() < until {
        thread::sleep(Duration::from_millis(20));
    }
    if matches!(child.try_wait(), Ok(None)) {
        let _ = child.kill();
    }
    let _ = child.wait();
}
pub fn probe() -> AudioStatus {
    let context = Context::current();
    let mut status = AudioStatus {
        enabled: true,
        state: "incomplete".into(),
        ..Default::default()
    };
    if context.socket("pipewire-0") {
        status.reused.push("pipewire".into());
    }
    if context.socket("pulse/native") {
        status.reused.push("pipewire-pulse".into());
    }
    if context.manager_present() {
        status.reused.push("wireplumber".into());
    }
    if status.reused.len() == 3 {
        status.state = "running".into();
    }
    status
}

pub fn change_volume(action: &str) -> Result<(u32, bool), String> {
    let context = Context::current();
    let args = control_args(action).ok_or("unknown audio action")?;
    let output = context.output("wpctl", args)?;
    if !output.status.success() {
        return Err("Audio volume command failed".into());
    }
    let target = if action == "mic-mute" {
        "@DEFAULT_AUDIO_SOURCE@"
    } else {
        "@DEFAULT_AUDIO_SINK@"
    };
    let output = context.output("wpctl", &["get-volume", target])?;
    if !output.status.success() {
        return Err("Could not read audio volume".into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let value = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .ok_or("Invalid audio volume")?;
    Ok((
        (value * 100.).round().clamp(0., 1000.) as u32,
        text.contains("MUTED"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    struct BusGuard(Child);
    impl Drop for BusGuard {
        fn drop(&mut self) {
            stop(&mut self.0);
        }
    }
    #[test]
    #[ignore = "starts an isolated real PipeWire stack; requires installed audio dependencies"]
    fn isolated_audio_stack_reuses_and_releases_only_its_processes() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("ember-audio-test-{}", std::process::id()));
        let runtime = root.join("run");
        let config = root.join("config");
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
        let wp = config.join("wireplumber/wireplumber.conf.d");
        std::fs::create_dir_all(&wp).unwrap();
        std::fs::write(wp.join("99-test.conf"),r#"wireplumber.profiles = { main = { hardware.audio = disabled hardware.bluetooth = disabled hardware.video-capture = disabled monitor.alsa = disabled monitor.alsa-midi = disabled monitor.bluez = disabled monitor.v4l2 = disabled monitor.libcamera = disabled support.logind = disabled support.reserve-device = disabled support.portal-permissionstore = disabled } }"#).unwrap();
        let mut context = Context {
            runtime: runtime.clone(),
            env: vec![],
        };
        for (k, v) in [
            ("XDG_RUNTIME_DIR", runtime.clone()),
            ("PIPEWIRE_RUNTIME_DIR", runtime.clone()),
            ("XDG_CONFIG_HOME", config),
            ("XDG_STATE_HOME", root.join("state")),
            ("XDG_CACHE_HOME", root.join("cache")),
        ] {
            context.env.push((k.into(), v.into_os_string()));
        }
        context.env.push((
            "PULSE_SERVER".into(),
            format!("unix:{}", runtime.join("pulse/native").display()).into(),
        ));
        let mut bus = BusGuard(
            context
                .command("dbus-daemon")
                .args([
                    "--session",
                    "--nofork",
                    &format!("--address=unix:path={}", runtime.join("bus").display()),
                ])
                .spawn()
                .unwrap(),
        );
        context.env.push((
            "DBUS_SESSION_BUS_ADDRESS".into(),
            format!("unix:path={}", runtime.join("bus").display()).into(),
        ));
        let first = AudioSession::start_in(context.clone());
        assert_eq!(first.status.state, "running", "{:?}", first.status);
        assert_eq!(first.status.started.len(), 3);
        let total_pss_kib: u64 = first
            .services
            .iter()
            .map(|service| {
                std::fs::read_to_string(format!("/proc/{}/smaps_rollup", service.process.id()))
                    .unwrap()
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("Pss:")
                            .and_then(|v| v.split_whitespace().next())
                            .and_then(|v| v.parse::<u64>().ok())
                    })
                    .unwrap_or(0)
            })
            .sum();
        println!(
            "Isolated idle audio stack PSS: {total_pss_kib} KiB (hardware monitors disabled)."
        );

        let second = AudioSession::start_in(context.clone());
        assert_eq!(second.status.state, "running");
        assert!(second.services.is_empty());
        assert_eq!(second.status.reused.len(), 3);
        drop(second);
        assert!(context.socket("pipewire-0"));
        let loaded = context
            .output(
                "pactl",
                &["load-module", "module-null-sink", "sink_name=ember_test"],
            )
            .unwrap();
        assert!(loaded.status.success());
        let sinks = context
            .output("pactl", &["list", "short", "sinks"])
            .unwrap();
        assert!(String::from_utf8_lossy(&sinks.stdout).contains("ember_test"));
        // Playback and record through a private virtual sink: no physical device is touched.
        context
            .output("pactl", &["set-default-sink", "ember_test"])
            .unwrap();
        assert!(context
            .output("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", "0.98"])
            .unwrap()
            .status
            .success());
        assert!(context
            .output("wpctl", control_args("volume-up").unwrap())
            .unwrap()
            .status
            .success());
        let volume = context
            .output("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .unwrap();
        assert!(String::from_utf8_lossy(&volume.stdout).contains("1.00"));
        for _ in 0..2 {
            assert!(context
                .output("wpctl", control_args("mute").unwrap())
                .unwrap()
                .status
                .success());
        }
        let mut recorder = context
            .command("parec")
            .args([
                "--device=ember_test.monitor",
                "--raw",
                "--format=s16le",
                "--rate=48000",
                "--channels=1",
                "--latency-msec=50",
            ])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut playback = context
            .command("pacat")
            .args([
                "--playback",
                "--device=ember_test",
                "--raw",
                "--format=s16le",
                "--rate=48000",
                "--channels=1",
                "--latency-msec=50",
            ])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = playback.stdin.take().unwrap();
        let writer = std::thread::spawn(move || {
            use std::io::Write;
            let data: Vec<u8> = (0..48000)
                .flat_map(|i| {
                    (((i as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin() * 4000.0) as i16)
                        .to_le_bytes()
                })
                .collect();
            let _ = input.write_all(&data);
        });
        let mut captured = recorder.stdout.take().unwrap();
        let reader = std::thread::spawn(move || {
            let mut data = Vec::new();
            let _ = captured.read_to_end(&mut data);
            data
        });
        std::thread::sleep(Duration::from_secs(3));
        stop(&mut playback);
        stop(&mut recorder);
        writer.join().unwrap();
        let data = reader.join().unwrap();
        let peak = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| i32::from(i16::from_le_bytes([s[0], s[1]])).abs())
            .max()
            .unwrap_or(0);
        assert!(
            peak > 100,
            "virtual PCM capture had {} bytes, peak {}",
            data.len(),
            peak
        );
        let pids: Vec<_> = first.services.iter().map(|s| s.process.id()).collect();
        drop(first);
        assert!(pids
            .iter()
            .all(|pid| !PathBuf::from(format!("/proc/{pid}")).exists()));
        stop(&mut bus.0);
        let _ = std::fs::remove_dir_all(&root);
        println!("Isolated audio: core + Pulse compatibility + WirePlumber, duplicate reuse, PCM loopback and owned-only cleanup passed.");
    }
}
