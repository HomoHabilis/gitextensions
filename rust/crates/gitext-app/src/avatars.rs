//! Port of `GitUI.Avatars`: author images in the revision grid and the commit details.
//!
//! Author initials on a coloured square (`InitialsAvatarProvider`) are drawn when there is no
//! image. With the GitHub / Gravatar provider, images are downloaded on background threads
//! (`AvatarDownloader`) and kept in a disk cache for some days (`FileSystemAvatarCache`), then
//! in textures for the session (`AvatarMemoryCache`). A miss is cached too, so an author
//! without an image is not asked for again until the cache entry expires.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use egui::{Color32, Rect, Sense, TextureHandle, Ui, Vec2};
use gitext_core::settings::{AppSettings, AvatarProvider};

/// Size of the downloaded images (`AppSettings.AvatarImageSize`).
const IMAGE_SIZE: u32 = 80;
/// Downloads running at the same time.
const WORKERS: usize = 4;

/// The initials palette (`AvatarAuthorInitialsPalette`).
const PALETTE: [Color32; 6] = [
    Color32::from_rgb(0x70, 0x80, 0x90), // SlateGray
    Color32::from_rgb(0x41, 0x69, 0xE1), // RoyalBlue
    Color32::from_rgb(0x80, 0x00, 0x80), // Purple
    Color32::from_rgb(0xFF, 0x45, 0x00), // OrangeRed
    Color32::from_rgb(0x00, 0x80, 0x80), // Teal
    Color32::from_rgb(0x6B, 0x8E, 0x23), // OliveDrab
];

/// The directory of the downloaded images (`AvatarImageCachePath`).
fn cache_dir() -> Option<PathBuf> {
    gitext_core::settings::config_dir().map(|d| d.join("Images"))
}

/// Deletes the downloaded images (`IAvatarCacheCleaner.ClearCacheAsync`).
pub fn clear_disk_cache() {
    if let Some(dir) = cache_dir() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    CLEARED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Set by [`clear_disk_cache`]: the textures are dropped too on the next frame.
static CLEARED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

enum State {
    Loading,
    Image(TextureHandle),
    /// No image: the initials are shown.
    Missing,
}

struct Request {
    email: String,
    cache_days: u32,
}

/// The author images of the session.
pub struct Avatars {
    provider: AvatarProvider,
    states: HashMap<String, State>,
    requests: Option<Sender<Request>>,
    results: Receiver<(String, Option<egui::ColorImage>)>,
    results_tx: Sender<(String, Option<egui::ColorImage>)>,
}

impl Default for Avatars {
    fn default() -> Self {
        let (results_tx, results) = mpsc::channel();
        Avatars { provider: AvatarProvider::None, states: HashMap::new(), requests: None, results, results_tx }
    }
}

impl Avatars {
    /// Follows the settings and takes the downloaded images; call once per frame.
    pub fn update(&mut self, ctx: &egui::Context, settings: &AppSettings) {
        if self.provider != settings.avatar_provider || CLEARED.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.provider = settings.avatar_provider;
            self.states.clear();
        }
        while let Ok((email, image)) = self.results.try_recv() {
            let state = match image {
                Some(img) => State::Image(ctx.load_texture(format!("avatar:{email}"), img, egui::TextureOptions::LINEAR)),
                None => State::Missing,
            };
            self.states.insert(email, state);
        }
    }

    /// The image of `email`, requesting it when not known yet.
    fn image(&mut self, ctx: &egui::Context, email: &str, cache_days: u32) -> Option<TextureHandle> {
        if self.provider == AvatarProvider::None {
            return None;
        }
        let key = email.trim().to_lowercase();
        if key.is_empty() {
            return None;
        }
        match self.states.get(&key) {
            Some(State::Image(t)) => return Some(t.clone()),
            Some(_) => return None,
            None => {}
        }
        self.states.insert(key.clone(), State::Loading);
        let tx = self.requests.get_or_insert_with(|| start_workers(ctx, self.results_tx.clone()));
        let _ = tx.send(Request { email: key, cache_days });
        None
    }

    /// Draws the avatar of an author: the image, or the initials.
    pub fn show(&mut self, ui: &mut Ui, email: &str, name: &str, size: f32, cache_days: u32) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
        if ui.is_rect_visible(rect) {
            self.paint(ui, rect, email, name, cache_days);
        }
        resp
    }

    /// Paints the avatar in `rect`.
    pub fn paint(&mut self, ui: &Ui, rect: Rect, email: &str, name: &str, cache_days: u32) {
        let rounding = (rect.width() * 0.15).round();
        match self.image(ui.ctx(), email, cache_days) {
            Some(tex) => {
                egui::Image::new((tex.id(), rect.size())).corner_radius(rounding).paint_at(ui, rect);
            }
            None => paint_initials(ui, rect, email, name, rounding),
        }
    }
}

/// Draws the initials of an author on the colour of their email (`InitialsAvatarProvider`).
pub fn paint_initials(ui: &Ui, rect: Rect, email: &str, name: &str, rounding: f32) {
    let (initials, index) = initials_and_color_index(email, name);
    let bg = PALETTE[index % PALETTE.len()];
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, rounding, bg);
    let font = egui::FontId::proportional((rect.height() * 0.45).max(6.0));
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, initials, font, contrast_color(bg));
}

/// Black or white, whichever reads on `bg` (`GetContrastColor` with the 0.5 threshold).
fn contrast_color(bg: Color32) -> Color32 {
    let luminance = (0.299 * bg.r() as f32 + 0.587 * bg.g() as f32 + 0.114 * bg.b() as f32) / 255.0;
    if luminance > 0.5 { Color32::BLACK } else { Color32::WHITE }
}

/// `GetDeterministicHashCode`: the same hash as upstream (over UTF-16 code units), so authors
/// keep their colour.
fn deterministic_hash(s: &str) -> usize {
    let mut hash: i32 = 23;
    for c in s.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(c as i32);
    }
    hash.wrapping_abs() as u32 as usize
}

/// `GetInitialsAndColorIndex`: the initials of the name (else of the email's user part) and
/// the palette index of the email.
pub fn initials_and_color_index(email: &str, name: &str) -> (String, usize) {
    const EMAIL_SEPARATORS: [char; 3] = ['.', '-', '_'];
    let parts: Vec<&str> = if !name.trim().is_empty() {
        name.trim().split(char::is_whitespace).collect()
    } else if !email.trim().is_empty() {
        email.split('@').next().unwrap_or_default().trim_start().split(EMAIL_SEPARATORS).collect()
    } else {
        return ("?".into(), 0);
    };
    (initials_from_names(&parts), deterministic_hash(email) % PALETTE.len())
}

/// `GetInitialsFromNames`.
fn initials_from_names(possible: &[&str]) -> String {
    let possible: Vec<&str> = possible.iter().copied().filter(|s| !s.trim().is_empty()).collect();
    let first = |s: &str| s.chars().next().unwrap_or('?');
    if possible.is_empty() {
        return "?".into();
    }
    let names: Vec<&str> = possible.iter().copied().filter(|s| first(s).is_alphanumeric()).collect();
    if names.is_empty() {
        return if possible.len() > 1 { format!("{}{}", first(possible[0]), first(possible[1])) } else { first(possible[0]).to_string() };
    }
    let name = names[0];
    let chars: Vec<char> = name.chars().collect();
    if names.len() == 1 {
        if chars.len() == 1 {
            return name.to_uppercase();
        }
        if chars[1].is_uppercase() {
            return format!("{}{}", chars[0].to_uppercase(), chars[1]);
        }
        let split: Vec<&str> = name.split(['.', '-', '_']).collect();
        if split.len() > 1 {
            return initials_from_names(&split);
        }
        let upper: Vec<char> = chars.iter().copied().filter(|c| c.is_uppercase()).collect();
        if upper.len() > 1 {
            return format!("{}{}", upper[0], upper[upper.len() - 1]);
        }
        return format!("{}{}", chars[0].to_uppercase(), chars[1]);
    }
    format!("{}{}", chars[0], first(names[names.len() - 1])).to_uppercase()
}

fn start_workers(ctx: &egui::Context, results: Sender<(String, Option<egui::ColorImage>)>) -> Sender<Request> {
    let (tx, rx) = mpsc::channel::<Request>();
    let rx = Arc::new(Mutex::new(rx));
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).try_proxy_from_env(true).user_agent("GitExtensions").build();
    for _ in 0..WORKERS {
        let (rx, results, ctx, agent) = (Arc::clone(&rx), results.clone(), ctx.clone(), agent.clone());
        std::thread::spawn(move || loop {
            let req = match rx.lock().unwrap().recv() {
                Ok(r) => r,
                Err(_) => return,
            };
            let image = load(&agent, &req.email, req.cache_days).and_then(|bytes| decode(&bytes));
            if results.send((req.email, image)).is_err() {
                return;
            }
            ctx.request_repaint();
        });
    }
    tx
}

/// The image bytes of `email`: from the disk cache when fresh, else downloaded (and cached).
/// `None` when the author has no image (also cached, as an empty file).
fn load(agent: &ureq::Agent, email: &str, cache_days: u32) -> Option<Vec<u8>> {
    let file = cache_dir().map(|d| d.join(format!("{}.{IMAGE_SIZE}px", md5_hex(email))));
    if let Some(f) = &file {
        let fresh = std::fs::metadata(f)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < Duration::from_secs(u64::from(cache_days) * 86_400));
        if fresh {
            let bytes = std::fs::read(f).ok()?;
            return (!bytes.is_empty()).then_some(bytes);
        }
    }
    let downloaded = image_urls(email).into_iter().find_map(|url| download(agent, &url));
    if let Some(f) = &file {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(f, downloaded.as_deref().unwrap_or_default());
    }
    downloaded
}

fn download(agent: &ureq::Agent, url: &str) -> Option<Vec<u8>> {
    let resp = agent.get(url).call().ok()?;
    let mut bytes = Vec::new();
    resp.into_reader().take(2 * 1024 * 1024).read_to_end(&mut bytes).ok()?;
    (!bytes.is_empty()).then_some(bytes)
}

/// The URLs tried for `email`, in order (`GithubAvatarProvider`, then `GravatarProvider` with
/// `d=404` so that a missing image is an error and the initials are shown).
fn image_urls(email: &str) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(user) = email.strip_suffix("@users.noreply.github.com") {
        // `12345+user@users.noreply.github.com` or `user@users.noreply.github.com`
        match user.split_once('+') {
            Some((id, _)) if id.chars().all(|c| c.is_ascii_digit()) => urls.push(format!("https://avatars.githubusercontent.com/u/{id}?s={IMAGE_SIZE}&v=4")),
            _ => urls.push(format!("https://avatars.githubusercontent.com/{user}?s={IMAGE_SIZE}&v=4")),
        }
    }
    urls.push(format!("https://www.gravatar.com/avatar/{}?s={IMAGE_SIZE}&d=404", md5_hex(email)));
    urls
}

fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

/// MD5 of the trimmed lower-case email, in hex (the Gravatar hash).
fn md5_hex(email: &str) -> String {
    md5(email.trim().to_lowercase().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// MD5 (RFC 1321): only used to name Gravatar images.
fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16,
        23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, v) in [a0, b0, c0, d0].iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_rfc_1321() {
        let hex = |s: &str| md5(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex("12345678901234567890123456789012345678901234567890123456789012345678901234567890"), "57edf4a22be3c955ac49da2e2107b67a");
        // the Gravatar documentation example
        assert_eq!(md5_hex(" MyEmailAddress@example.com "), "0bc83cb571cd1c50ba6f3e8a78ef1346");
    }

    /// The cases of upstream `InitialsAvatarGeneratorTests`.
    #[test]
    fn initials_as_upstream() {
        let cases = [
            ("albert.einstein@noreply.com", "albert einstein", "AE"),
            ("albert.einstein@noreply.com", "albert z middlename einstein (k)", "AE"),
            ("albert.einstein@noreply.com", "albert einstein (Nobel_price_winner)", "AE"),
            ("albert.einstein@noreply.com", "EINSTEIN, Albert middlename (Nobel_price_winner)", "EM"),
            ("albert.einstein@noreply.com", "albert", "Al"),
            ("albert.einstein@noreply.com", "", "AE"),
            ("albert.z.einstein@noreply.com", "", "AE"),
            ("albert_z_einstein@noreply.com", "", "AE"),
            ("albert.bose-einstein@noreply.com", "", "AE"),
            ("albert", "", "Al"),
            ("albert.einstein", "", "AE"),
            ("", "albert.einstein", "AE"),
            ("", "AlbertEinstein", "AE"),
            ("", "A-Einstein", "AE"),
            ("", "Alb-Einstein", "AE"),
            ("", "AEinstein", "AE"),
            ("1-pass@noreply.com", "1-pass", "1P"),
            ("", "1-pass", "1P"),
            ("1-pass@noreply.com", "", "1P"),
            ("/*+=/°¨", "'µ*", "'"),
            ("/*+=/°¨", "'µ *", "'*"),
            ("", "", "?"),
        ];
        for (email, name, expected) in cases {
            assert_eq!(initials_and_color_index(email, name).0, expected, "{email:?} {name:?}");
        }
    }

    #[test]
    fn colour_follows_the_email() {
        let (_, a) = initials_and_color_index("a@b.c", "Ann");
        let (_, b) = initials_and_color_index("a@b.c", "Other name");
        assert_eq!(a, b);
        assert!(a < PALETTE.len());
        // C# `GetDeterministicHashCode("a")` = 23 * 31 + 97
        assert_eq!(deterministic_hash("a"), 810);
    }

    #[test]
    fn github_noreply_addresses_use_github() {
        let urls = image_urls("123+octo@users.noreply.github.com");
        assert_eq!(urls[0], "https://avatars.githubusercontent.com/u/123?s=80&v=4");
        assert!(urls[1].starts_with("https://www.gravatar.com/avatar/"));
        assert_eq!(image_urls("octo@users.noreply.github.com")[0], "https://avatars.githubusercontent.com/octo?s=80&v=4");
        assert_eq!(image_urls("a@b.c").len(), 1);
    }

    #[test]
    fn decodes_png() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2, 3, image::Rgba([1, 2, 3, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let img = decode(&png).unwrap();
        assert_eq!(img.size, [2, 3]);
        assert!(decode(b"not an image").is_none());
    }
}
