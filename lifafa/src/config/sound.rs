//! The new-mail sound (#292): sound.toml and the copy of a custom sound
//! file.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::{config_file, data_base, read_toml, save_plain_toml};

/// Where the custom new-mail sound is kept (#292). It is a copy, so the
/// original can be moved or deleted, and inside the Flatpak it stays readable
/// after the file chooser's grant lapses. The file keeps its own name, which
/// is what Settings shows; there is never more than one.
fn custom_sound_dir() -> Option<PathBuf> {
    data_base().map(|d| d.join("hylki").join("notification-sound"))
}

/// The custom new-mail sound file, if one has been chosen.
pub(crate) fn custom_sound() -> Option<PathBuf> {
    std::fs::read_dir(custom_sound_dir()?)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.is_file()
                && !p
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        })
}

/// Make a copy of `src` the custom new-mail sound, in place of any earlier one.
pub(crate) fn set_custom_sound(src: &Path) -> std::io::Result<PathBuf> {
    let dir = custom_sound_dir()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no data directory"))?;
    let name = src
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a file"))?;
    // Copied beside the old one first, so a failed copy leaves the old
    // sound playing rather than none.
    std::fs::create_dir_all(&dir)?;
    let part = dir.join(".incoming");
    let bytes = std::fs::read(src)?;
    std::fs::write(&part, without_id3v2(&bytes))?;
    for entry in std::fs::read_dir(&dir)?.flatten() {
        if entry.file_name() != ".incoming" {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let dest = dir.join(name);
    std::fs::rename(&part, &dest)?;
    Ok(dest)
}

/// `bytes` without the ID3v2 tags in front. An MP3 that starts with one
/// makes GTK's media file abort the whole app inside GStreamer's decodebin3
/// (#397), while the same audio without the tag plays. The tag only holds
/// the title, artist and cover art, none of which a notification needs.
fn without_id3v2(mut bytes: &[u8]) -> &[u8] {
    while bytes.len() >= 10
        && bytes.starts_with(b"ID3")
        && bytes[6..10].iter().all(|b| b & 0x80 == 0)
    {
        // A syncsafe size: seven bits a byte, not counting the 10-byte
        // header, nor the 10-byte footer when flag 0x10 says there is one.
        let size = bytes[6..10]
            .iter()
            .fold(0usize, |n, b| (n << 7) | usize::from(*b));
        let footer = if bytes[5] & 0x10 != 0 { 10 } else { 0 };
        let Some(rest) = bytes.get(10 + size + footer..) else {
            break;
        };
        bytes = rest;
    }
    bytes
}

/// Take the ID3v2 tag off a custom sound copied in before #397, so the next
/// new mail does not take the app down. Cheap when there is none: it reads
/// three bytes.
fn repair_custom_sound(path: &Path) {
    use std::io::Read;
    let mut head = [0u8; 3];
    let tagged = std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && &head == b"ID3";
    if !tagged {
        return;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let audio = without_id3v2(&bytes);
    if audio.len() == bytes.len() {
        return;
    }
    let part = path.with_file_name(".incoming");
    match std::fs::write(&part, audio).and_then(|()| std::fs::rename(&part, path)) {
        Ok(()) => tracing::info!("new-mail sound: took the ID3v2 tag off {}", path.display()),
        Err(e) => tracing::warn!(
            "new-mail sound: could not take the ID3v2 tag off {}: {e}",
            path.display()
        ),
    }
}

/// The built-in new-mail sounds: GNOME's four alert sounds (data/sounds/),
/// so there is something to hear without hunting for a file. The desktop's
/// own sound theme is out of reach: the GNOME runtime ships none, and the
/// Flatpak cannot see the host's /usr/share/sounds.
pub(crate) const BUILTIN_SOUNDS: [&str; 4] = ["click", "hum", "string", "swing"];

/// The `sound` value naming the custom file.
pub(crate) const CUSTOM_SOUND: &str = "custom";

/// Something to play: a built-in sound's resource path, or the custom file.
pub(crate) enum SoundSource {
    Resource(String),
    File(PathBuf),
}

/// The new-mail sound settings (#292). Stored in sound.toml, like focus.toml,
/// so privacy.toml's long save call stays as it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NewMailSound {
    pub(crate) enabled: bool,
    /// One of `BUILTIN_SOUNDS`, or `CUSTOM_SOUND`.
    pub(crate) sound: String,
    /// Play it while the window is in front too, where no notification is
    /// shown (#337). Off, the sound comes only with the notification.
    #[serde(default)]
    pub(crate) when_focused: bool,
}

impl NewMailSound {
    /// What `sound` names. An unknown name is the first built-in; a custom
    /// sound whose file has gone is nothing.
    pub(crate) fn source(&self) -> Option<SoundSource> {
        if self.sound == CUSTOM_SOUND {
            let path = custom_sound()?;
            repair_custom_sound(&path);
            return Some(SoundSource::File(path));
        }
        let name = BUILTIN_SOUNDS
            .iter()
            .find(|n| **n == self.sound)
            .unwrap_or(&BUILTIN_SOUNDS[0]);
        Some(SoundSource::Resource(format!(
            "/co/hyprlab/Hylki/sounds/{name}.ogg"
        )))
    }
}

fn sound_path() -> Option<PathBuf> {
    config_file("sound.toml")
}

/// The saved new-mail sound settings. Without sound.toml the sound is off,
/// unless a custom file was chosen before the switch existed: that stays on.
pub(crate) fn load_new_mail_sound() -> NewMailSound {
    if let Some(saved) = read_toml::<NewMailSound>(sound_path()) {
        return saved;
    }
    let custom = custom_sound().is_some();
    NewMailSound {
        enabled: custom,
        sound: if custom {
            CUSTOM_SOUND
        } else {
            BUILTIN_SOUNDS[0]
        }
        .to_string(),
        when_focused: false,
    }
}

pub(crate) fn save_new_mail_sound(sound: &NewMailSound) {
    save_plain_toml(sound_path(), sound);
}

/// What to play for new mail, if anything.
pub(crate) fn new_mail_sound() -> Option<SoundSource> {
    let sound = load_new_mail_sound();
    if sound.enabled {
        sound.source()
    } else {
        None
    }
}

/// Whether the new-mail sound plays while the window is in front as well
/// (#337).
pub(crate) fn sound_when_focused() -> bool {
    let sound = load_new_mail_sound();
    sound.enabled && sound.when_focused
}

#[cfg(test)]
mod sound_tests {
    use super::without_id3v2;

    #[test]
    fn id3v2_tags_come_off() {
        let audio = [0xFFu8, 0xFB, 0x90, 0x64];
        // An ID3v2.4 tag of 1024 bytes (syncsafe 0x00 0x00 0x08 0x00), as in #397.
        let mut tagged = b"ID3\x04\x00\x00\x00\x00\x08\x00".to_vec();
        tagged.extend(std::iter::repeat_n(0, 1024));
        tagged.extend(audio);
        assert_eq!(without_id3v2(&tagged), audio);
        // With a footer, and two tags in a row.
        let mut footed = b"ID3\x04\x00\x10\x00\x00\x00\x02xx".to_vec();
        footed.extend(b"3DI\x04\x00\x10\x00\x00\x00\x02");
        footed.extend(b"ID3\x03\x00\x00\x00\x00\x00\x00");
        footed.extend(audio);
        assert_eq!(without_id3v2(&footed), audio);
        // Untagged audio, a truncated tag and a broken size stay as they are.
        assert_eq!(without_id3v2(&audio), audio);
        let short = b"ID3\x04\x00\x00\x00\x00\x08\x00abc";
        assert_eq!(without_id3v2(short), short);
        let bad = b"ID3\x04\x00\x00\x80\x00\x00\x00abc";
        assert_eq!(without_id3v2(bad), bad);
    }
}
