use smithay::reexports::drm::control::{Mode, ModeFlags, ModeTypeFlags};
use std::{
    fs::File,
    os::fd::{AsFd, BorrowedFd},
    time::Duration,
};

pub fn refresh_millihz(mode: &Mode) -> i32 {
    let total = u64::from(mode.hsync().2) * u64::from(mode.vsync().2);
    if total == 0 {
        return (mode.vrefresh() as i32 * 1000).max(1);
    }
    let mut refresh = u64::from(mode.clock()) * 1_000_000 / total;
    if mode.flags().contains(ModeFlags::INTERLACE) {
        refresh *= 2;
    }
    if mode.flags().contains(ModeFlags::DBLSCAN) {
        refresh /= 2;
    }
    if mode.vscan() > 1 {
        refresh /= u64::from(mode.vscan());
    }
    refresh.clamp(1, i32::MAX as u64) as i32
}

/// Select rate at the preferred resolution; never lower the resolution to chase Hz.
pub fn choose_mode(modes: &[Mode], target_hz: f64) -> Option<Mode> {
    let preferred = modes
        .iter()
        .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or(modes.first())?;
    if target_hz == 0.0 {
        return Some(*preferred);
    }
    modes
        .iter()
        .filter(|m| m.size() == preferred.size())
        .min_by(|a, b| {
            let da = (f64::from(refresh_millihz(a)) / 1000.0 - target_hz).abs();
            let db = (f64::from(refresh_millihz(b)) / 1000.0 - target_hz).abs();
            da.total_cmp(&db)
        })
        .copied()
}

pub fn frame_interval(refresh_millihz: i32) -> Duration {
    Duration::from_nanos(1_000_000_000_000u64 / refresh_millihz.max(1) as u64)
}

pub fn active_interval(state: &crate::Ember) -> Duration {
    frame_interval(
        state
            .space
            .outputs()
            .next()
            .and_then(|o| o.current_mode())
            .map(|m| m.refresh)
            .unwrap_or(60_000),
    )
}

struct ReadOnlyDrm(File);
impl AsFd for ReadOnlyDrm {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}
impl smithay::reexports::drm::Device for ReadOnlyDrm {}
impl smithay::reexports::drm::control::Device for ReadOnlyDrm {}

/// Query connector modes without creating a DRM master or changing the display.
pub fn list_modes(target_hz: f64) -> Result<(), Box<dyn std::error::Error>> {
    use smithay::reexports::drm::control::{connector::State, Device};
    let path = std::env::var("EMBER_DRM_DEVICE").unwrap_or_else(|_| "/dev/dri/card0".into());
    let device = ReadOnlyDrm(File::open(&path)?);
    let mut outputs = vec![];
    for handle in device.resource_handles()?.connectors() {
        let info = device.get_connector(*handle, true)?;
        if info.state() != State::Connected {
            continue;
        }
        let chosen = choose_mode(info.modes(), target_hz);
        let modes: Vec<_> = info
            .modes()
            .iter()
            .map(|m| {
                serde_json::json!({"width":m.size().0,"height":m.size().1,
            "refresh_hz": f64::from(refresh_millihz(m))/1000.0,"selected":Some(*m)==chosen})
            })
            .collect();
        outputs.push(serde_json::json!({"connector":format!("{:?}-{}",info.interface(),info.interface_id()),"modes":modes}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"device":path,"requested_hz":target_hz,"outputs":outputs})
        )?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use drm_ffi::drm_mode_modeinfo;
    fn mode(clock: u32, preferred: bool) -> Mode {
        let raw = drm_mode_modeinfo {
            clock,
            hdisplay: 1920,
            hsync_start: 2008,
            hsync_end: 2052,
            htotal: 2200,
            vdisplay: 1080,
            vsync_start: 1084,
            vsync_end: 1089,
            vtotal: 1125,
            type_: if preferred { 8 } else { 0 },
            ..Default::default()
        };
        raw.into()
    }
    #[test]
    fn selects_nominal_144_instead_of_preferred_60_or_higher_165() {
        let modes = [mode(148500, true), mode(408375, false), mode(356350, false)];
        let chosen = choose_mode(&modes, 144.0).unwrap();
        assert_eq!(chosen.clock(), 356350);
        assert!((f64::from(refresh_millihz(&chosen)) / 1000.0 - 144.0).abs() < 0.1);
        assert_eq!(choose_mode(&modes, 0.0).unwrap().clock(), 148500);
    }
    #[test]
    fn frame_budget_follows_display_rate() {
        assert!(frame_interval(143_980) < Duration::from_millis(7));
        assert_eq!(frame_interval(60_000).as_nanos(), 16_666_666);
        assert!(choose_mode(&[], 144.0).is_none());
    }
}
