//! Optional native helper compiled against a read-only rdwm source checkout.
//! Reuses rdwm-deco; never replaces its renderer or starts a window manager.
use rdwm_deco::*;
use std::{fs, path::Path};

fn render(cfg: &DecorationConfig, path: &Path, scale: f32, hovers: bool) {
    let cfg = cfg.scaled(scale);
    let (w, h, gap) = (680u32 * scale as u32, cfg.height_px(), (6.0 * scale) as u32);
    let states = if hovers { vec![(true, None), (false, None), (true, Some(Part::Close)), (true, Some(Part::Minimize)), (true, Some(Part::Zoom))] } else { vec![(true, None), (false, None)] };
    let mut bytes = vec![0; Surface::len_for(w, (h + gap) * states.len() as u32)];
    let mut deco = Deco::new(&cfg);
    for (i, (focused, hover)) in states.iter().enumerate() {
        let offset = i * (h + gap) as usize * w as usize * 4;
        let mut surface = Surface::new(&mut bytes[offset..], w, h, w as usize * 4).unwrap();
        deco.draw_titlebar(&TitlebarState { title: if *focused { "Focused sample window" } else { "Unfocused sample window" }, focused: *focused, hover: *hover }, &mut surface);
    }
    for (part, (x, y, bw, bh)) in titlebar_buttons(&cfg, w as i32) {
        assert!(bw > 0 && bh > 0 && x >= 0 && x + bw <= w as i32 && y >= 0 && y + bh <= h as i32);
        assert_eq!(titlebar_hit_with(&cfg, w as i32, x + bw / 2, y + bh / 2), Some(part));
    }
    if cfg.has_buttons() {
        assert_eq!(titlebar_buttons(&cfg, w as i32).len(), 3);
    }
    // BGRA premultiplied -> RGBA straight for a true transparent native preview.
    for p in bytes.chunks_exact_mut(4) {
        p.swap(0, 2);
        let a = u32::from(p[3]);
        if a > 0 {
            for c in &mut p[..3] {
                *c = (u32::from(*c) * 255 / a).min(255) as u8;
            }
        }
    }
    let mut encoder = png::Encoder::new(fs::File::create(path).unwrap(), w, (h + gap) * states.len() as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&bytes).unwrap();
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("metadata") => {
            let rows:Vec<_>=Style::ALL.into_iter().map(|style| {
                let cfg=DecorationConfig{style,..Default::default()};let m=cfg.metrics();let s=style.spec();
                serde_json::json!({"id":style.name(),"description":s.desc,"side":format!("{:?}",s.side),"shape":format!("{:?}",s.shape),"look":format!("{:?}",s.look),
                    "defaults":{"height":m.height,"radius":m.radius,"border_px":m.border,"title_align":if m.align==Align::Center{"center"}else{"left"},"button_size":m.size,"button_gap":m.gap,"button_margin":m.margin}})
            }).collect();
            println!("{}", serde_json::to_string_pretty(&rows).unwrap());
        }
        Some("theme") => {
            let root: toml::Value = fs::read_to_string(&args[1]).unwrap().parse().unwrap();
            let cfg: DecorationConfig = root.get("decoration").cloned().unwrap_or(toml::Value::Table(Default::default())).try_into().unwrap();
            // Border-only is intentionally button-free. Do not assert three buttons there.
            render(&cfg, Path::new(&args[2]), 1.0, false);
        }
        Some("catalogue") => {
            let dir = Path::new(&args[1]);
            fs::create_dir_all(dir).unwrap();
            let mut count = 0;
            for style in Style::ALL {
                for light in [false, true] {
                    for scale in [1.0, 2.0] {
                        let cfg = DecorationConfig {
                            style,
                            buttons: true,
                            zoom_button: ZoomMode::Maximize,
                            focused_bg: Some(Rgba::hex(if light { 0xf2f3f5 } else { 0x23262d })),
                            focused_fg: Some(Rgba::hex(if light { 0x18202a } else { 0xf1f3f8 })),
                            unfocused_bg: Some(Rgba::hex(if light { 0xe3e6e9 } else { 0x181b22 })),
                            unfocused_fg: Some(Rgba::hex(if light { 0x59626f } else { 0xb8becb })),
                            ..Default::default()
                        };
                        render(&cfg, &dir.join(format!("{}-{}-{}x.png", style.name(), if light { "light" } else { "dark" }, scale as i32)), scale, true);
                        count += 1;
                    }
                }
            }
            println!("{count} native style/mode/scale sheets; all caption button centres hit their actual actions");
        }
        _ => panic!("theme CONFIG PNG | catalogue DIR"),
    }
}
