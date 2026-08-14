//! Staged composer attachments, grok-style inline chips: each staged
//! image owns a literal `[image N]` token living *in the draft text*.
//! The chip is pure rendering over that token — cursor motion, backspace,
//! kills, and history recall edit it like ordinary text, and the tray
//! reconciles itself to whatever tokens survive. Hovering a chip (or
//! parking the cursor on one) pops a preview with basic metadata.
//!
//! Pure model; rendering, hover hit-tests, and reconcile policy live in
//! `ui` and `app`.

use std::sync::Arc;

pub const MAX_STAGED: usize = 8;

/// Composer preview thumbnails get kitty image ids far above the
/// transcript's `image_seq` counters so the two sync pools never collide.
pub const KITTY_ID_BASE: u32 = 0x4000_0000;

pub struct Attachment {
    pub id: u32,
    /// The literal draft-text token addressing this image, `[image N]`.
    /// `N` is the staging sequence number — stable, never reindexed.
    pub token: String,
    pub name: String,
    pub path: String,
    pub media_type: String,
    pub data: Arc<[u8]>,
}

impl Attachment {
    /// Kitty `f=100` renders PNG payloads only (clipboard screenshots
    /// always are); other formats preview as metadata text.
    pub fn is_png(&self) -> bool {
        self.data.starts_with(b"\x89PNG\r\n\x1a\n")
    }
}

/// The staged set. Order is staging order; the draft's token order decides
/// send order.
#[derive(Default)]
pub struct Staged {
    items: Vec<Attachment>,
    seq: u32,
}

impl Staged {
    /// Append one image and mint its draft token; `Err` when full.
    pub fn add(
        &mut self,
        name: String,
        path: String,
        media_type: String,
        data: Vec<u8>,
    ) -> Result<&Attachment, &'static str> {
        if self.items.len() >= MAX_STAGED {
            return Err("attachment tray is full — send or remove an [image] chip first");
        }
        self.seq += 1;
        self.items.push(Attachment {
            id: KITTY_ID_BASE + self.seq,
            token: format!("[image {}]", self.seq),
            name,
            path,
            media_type,
            data: Arc::from(data),
        });
        Ok(self.items.last().expect("just pushed"))
    }

    pub fn remove(&mut self, idx: usize) -> Option<Attachment> {
        (idx < self.items.len()).then(|| self.items.remove(idx))
    }

    /// Drop every attachment whose token no longer appears in `text`
    /// (edited away, draft cleared, history recall …). Returns how many
    /// were dropped.
    pub fn reconcile(&mut self, text: &str) -> usize {
        let before = self.items.len();
        self.items.retain(|a| text.contains(&a.token));
        before - self.items.len()
    }

    pub fn drain(&mut self) -> Vec<Attachment> {
        std::mem::take(&mut self.items)
    }

    /// Tray size (tests and future badges; prod reads go via `iter`).
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Attachment> {
        self.items.iter()
    }

    pub fn get(&self, idx: usize) -> Option<&Attachment> {
        self.items.get(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged_with(n: usize) -> Staged {
        let mut s = Staged::default();
        for i in 0..n {
            s.add(
                format!("img-{i}.png"),
                "clipboard".into(),
                "image/png".into(),
                vec![i as u8],
            )
            .unwrap();
        }
        s
    }

    #[test]
    fn add_mints_stable_unique_tokens_and_caps() {
        let mut s = staged_with(MAX_STAGED);
        assert!(s
            .add("x".into(), "p".into(), "image/png".into(), vec![])
            .is_err());
        let tokens: Vec<&str> = s.iter().map(|a| a.token.as_str()).collect();
        assert_eq!(tokens[0], "[image 1]");
        assert_eq!(tokens[7], "[image 8]");
        // Tokens never reindex after a removal.
        s.remove(0);
        assert_eq!(s.get(0).unwrap().token, "[image 2]");
    }

    #[test]
    fn reconcile_drops_attachments_whose_token_left_the_text() {
        let mut s = staged_with(3);
        let dropped = s.reconcile("keep [image 1] and [image 3] only");
        assert_eq!(dropped, 1);
        let tokens: Vec<&str> = s.iter().map(|a| a.token.as_str()).collect();
        assert_eq!(tokens, ["[image 1]", "[image 3]"]);
        // A broken token counts as gone.
        let dropped = s.reconcile("[image 1 …oops [image 3]");
        assert_eq!(dropped, 1);
        assert_eq!(s.len(), 1);
        assert_eq!(s.reconcile(""), 1);
        assert!(s.is_empty());
    }

    #[test]
    fn png_sniffing_reads_the_magic() {
        let mut s = Staged::default();
        s.add(
            "a.png".into(),
            "p".into(),
            "image/png".into(),
            b"\x89PNG\r\n\x1a\nrest".to_vec(),
        )
        .unwrap();
        s.add(
            "b.jpg".into(),
            "p".into(),
            "image/jpeg".into(),
            vec![0xff, 0xd8],
        )
        .unwrap();
        assert!(s.get(0).unwrap().is_png());
        assert!(!s.get(1).unwrap().is_png());
    }
}
