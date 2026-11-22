use std::{borrow::Cow,fs,io::{self,Write}};
fn main()->Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=std::env::args().collect();
    let mut clipboard=arboard::Clipboard::new()?;
    match args.get(1).map(String::as_str) {
        Some("image")=>{let image=clipboard.get_image()?;fs::write(&args[2],&image.bytes)?;println!("{{\"width\":{},\"height\":{},\"bytes\":{}}}",image.width,image.height,image.bytes.len());}
        Some("text")=>{let text=clipboard.get_text()?;fs::write(&args[2],text)?;println!("ok");}
        Some("set-image")=>{let width=args[2].parse()?;let height=args[3].parse()?;let bytes=fs::read(&args[4])?;clipboard.set_image(arboard::ImageData{width,height,bytes:Cow::Owned(bytes)})?;println!("ready");io::stdout().flush()?;let mut line=String::new();io::stdin().read_line(&mut line)?;}
        Some("set-text")=>{clipboard.set_text(args[2].clone())?;println!("ready");io::stdout().flush()?;let mut line=String::new();io::stdin().read_line(&mut line)?;}
        _=>return Err("expected image/text/set-image/set-text".into()),
    }
    Ok(())
}
