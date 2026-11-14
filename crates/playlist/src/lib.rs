//! The playlist model behind the playlist editor: entries, selection, editing, sorting and the
//! next/previous rules for shuffle and repeat. No UI and no playback here.
//!
//! Selection follows Webamp's reducer (`js/reducers/playlist.ts`): a click selects one row,
//! ctrl-click toggles a row, shift-click selects the range from the last clicked row. Next and
//! previous follow Webamp's `getNextTrackId`: repeat wraps, shuffle picks a random other entry,
//! and otherwise playback stops at either end.

mod url;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub use url::{is_video_id, parse_link, parse_links, parse_video_id, Link};

/// One playlist row: a YouTube video plus what we know about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub video_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub duration_secs: Option<u32>,
}

impl Entry {
    /// An entry that knows only its id. The title is the id until metadata arrives.
    pub fn from_id(video_id: &str) -> Entry {
        Entry {
            video_id: video_id.to_string(),
            title: video_id.to_string(),
            artist: String::new(),
            duration_secs: None,
        }
    }
}

/// The entries, the current entry (the one loaded or playing), the selected rows and the anchor
/// row that shift-click ranges start from. Indices are always kept in range.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Playlist {
    entries: Vec<Entry>,
    current: Option<usize>,
    selected: BTreeSet<usize>,
    anchor: Option<usize>,
}

/// `"{n}. {artist} - {title}"`, or `"{n}. {title}"` when there is no artist. An empty title
/// falls back to the video id, so a row always has text.
pub fn format_line(number: usize, entry: &Entry) -> String {
    let title = if entry.title.trim().is_empty() {
        entry.video_id.as_str()
    } else {
        entry.title.trim()
    };
    let artist = entry.artist.trim();
    if artist.is_empty() {
        format!("{number}. {title}")
    } else {
        format!("{number}. {artist} - {title}")
    }
}

/// A random index in `0..n` other than `current`. With one entry that is `0`.
fn random_other(n: usize, current: Option<usize>, rng: &mut fastrand::Rng) -> usize {
    if n == 1 {
        return 0;
    }
    match current {
        Some(c) if c < n => {
            let r = rng.usize(0..n - 1);
            if r >= c {
                r + 1
            } else {
                r
            }
        }
        _ => rng.usize(0..n),
    }
}

impl Playlist {
    /// A playlist holding `entries`, with nothing current or selected.
    pub fn from_entries(entries: Vec<Entry>) -> Playlist {
        Playlist {
            entries,
            ..Playlist::default()
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, index: usize) -> Option<&Entry> {
        self.entries.get(index)
    }

    /// Mutable access to one entry, for filling in metadata. Does not change indices.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Entry> {
        self.entries.get_mut(index)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn current_entry(&self) -> Option<&Entry> {
        self.current.and_then(|i| self.entries.get(i))
    }

    /// Sets the current entry. An index out of range clears it.
    pub fn set_current(&mut self, index: Option<usize>) {
        self.current = index.filter(|&i| i < self.entries.len());
    }

    pub fn selected(&self) -> &BTreeSet<usize> {
        &self.selected
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    /// The first entry with this video id.
    pub fn index_of(&self, video_id: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.video_id == video_id)
    }

    /// The row text for entry `index`, numbered from 1, or `None` when out of range.
    pub fn display_line(&self, index: usize) -> Option<String> {
        self.entries.get(index).map(|e| format_line(index + 1, e))
    }

    /// Sum of known durations, in seconds. Unknown durations count as zero.
    pub fn total_secs(&self) -> u64 {
        self.entries
            .iter()
            .map(|e| u64::from(e.duration_secs.unwrap_or(0)))
            .sum()
    }

    /// Sum of known durations of the selected rows.
    pub fn selected_secs(&self) -> u64 {
        self.selected
            .iter()
            .filter_map(|&i| self.entries.get(i))
            .map(|e| u64::from(e.duration_secs.unwrap_or(0)))
            .sum()
    }

    /// Appends an entry and returns its index.
    pub fn add(&mut self, entry: Entry) -> usize {
        self.entries.push(entry);
        self.entries.len() - 1
    }

    /// Inserts an entry at `at` (clamped to the end). Current and selected rows move with their
    /// entries.
    pub fn insert(&mut self, at: usize, entry: Entry) {
        let at = at.min(self.entries.len());
        self.entries.insert(at, entry);
        let shift = |i: usize| if i >= at { i + 1 } else { i };
        self.current = self.current.map(shift);
        self.anchor = self.anchor.map(shift);
        self.selected = self.selected.iter().map(|&i| shift(i)).collect();
    }

    /// Removes every entry for which `keep(index)` is false. Indices of the survivors shift down,
    /// and selection and the current entry follow their entries. Removing the current entry
    /// leaves nothing current.
    fn retain_by(&mut self, keep: impl Fn(usize) -> bool) {
        let mut map: Vec<Option<usize>> = Vec::with_capacity(self.entries.len());
        let mut next = 0;
        for i in 0..self.entries.len() {
            if keep(i) {
                map.push(Some(next));
                next += 1;
            } else {
                map.push(None);
            }
        }
        let mut i = 0;
        self.entries.retain(|_| {
            let kept = map[i].is_some();
            i += 1;
            kept
        });
        self.current = self.current.and_then(|c| map[c]);
        self.anchor = self.anchor.and_then(|a| map[a]);
        self.selected = self.selected.iter().filter_map(|&s| map[s]).collect();
    }

    /// Removes the selected rows (Winamp's REM ▸ SEL). Nothing is selected afterwards.
    pub fn remove_selected(&mut self) {
        let selected = self.selected.clone();
        self.retain_by(|i| !selected.contains(&i));
    }

    /// Keeps only the selected rows (REM ▸ CROP). Every remaining row is then selected.
    pub fn crop(&mut self) {
        let selected = self.selected.clone();
        self.retain_by(|i| selected.contains(&i));
    }

    /// Removes everything (REM ▸ ALL, and LIST ▸ NEW).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = None;
        self.selected.clear();
        self.anchor = None;
    }

    /// Selects every row (SEL ▸ ALL).
    pub fn select_all(&mut self) {
        self.selected = (0..self.entries.len()).collect();
    }

    /// Clears the selection (SEL ▸ CLR). The anchor stays, as in Webamp.
    pub fn select_none(&mut self) {
        self.selected.clear();
    }

    /// Selects the rows that were not selected (SEL ▸ INV).
    pub fn invert_selection(&mut self) {
        self.selected = (0..self.entries.len())
            .filter(|i| !self.selected.contains(i))
            .collect();
    }

    /// A plain click: selects only row `index` and makes it the anchor.
    pub fn click(&mut self, index: usize) {
        if index < self.entries.len() {
            self.selected = BTreeSet::from([index]);
            self.anchor = Some(index);
        }
    }

    /// Ctrl-click: toggles row `index`, and makes it the anchor (as Winamp 2 does, even when the
    /// row was just deselected).
    pub fn ctrl_click(&mut self, index: usize) {
        if index >= self.entries.len() {
            return;
        }
        if !self.selected.remove(&index) {
            self.selected.insert(index);
        }
        self.anchor = Some(index);
    }

    /// Shift-click: selects the rows from the anchor to `index`, replacing the selection. Without
    /// an anchor it does nothing. The anchor does not move.
    pub fn shift_click(&mut self, index: usize) {
        let Some(anchor) = self.anchor else {
            return;
        };
        if index >= self.entries.len() {
            return;
        }
        let (lo, hi) = (anchor.min(index), anchor.max(index));
        self.selected = (lo..=hi).collect();
    }

    /// Moves the selected rows by `delta` rows, for drag-reordering. The selected rows keep their
    /// order and end up contiguous, starting at the first selected row's index plus `delta`
    /// (clamped to the list).
    pub fn move_selected(&mut self, delta: isize) {
        if delta == 0 || self.selected.is_empty() {
            return;
        }
        let first = *self.selected.iter().next().expect("selection is not empty");
        let mut moved = Vec::with_capacity(self.selected.len());
        let mut rest = Vec::with_capacity(self.entries.len() - self.selected.len());
        for i in 0..self.entries.len() {
            if self.selected.contains(&i) {
                moved.push(i);
            } else {
                rest.push(i);
            }
        }
        // Rows before `first` are all unselected, so `first` is also its position in `rest`.
        let at = (first as isize + delta).clamp(0, rest.len() as isize) as usize;
        let order: Vec<usize> = rest[..at]
            .iter()
            .chain(moved.iter())
            .chain(rest[at..].iter())
            .copied()
            .collect();
        self.apply_order(&order);
        self.selected = (at..at + moved.len()).collect();
    }

    /// Reorders the entries so that new row `k` is old row `order[k]`, and moves the current and
    /// anchor rows with their entries. `order` must be a permutation of `0..len`.
    fn apply_order(&mut self, order: &[usize]) {
        let mut old: Vec<Option<Entry>> = std::mem::take(&mut self.entries)
            .into_iter()
            .map(Some)
            .collect();
        let mut inverse = vec![0; order.len()];
        for (new, &from) in order.iter().enumerate() {
            inverse[from] = new;
            self.entries
                .push(old[from].take().expect("order is a permutation"));
        }
        self.current = self.current.map(|c| inverse[c]);
        self.anchor = self.anchor.map(|a| inverse[a]);
        self.selected = self.selected.iter().map(|&s| inverse[s]).collect();
    }

    /// Sorts by title, then artist. Case-insensitive and stable.
    pub fn sort_by_title(&mut self) {
        self.sort_by_key_pair(|e| (e.title.to_lowercase(), e.artist.to_lowercase()));
    }

    /// Sorts by artist, then title. Case-insensitive and stable.
    pub fn sort_by_artist(&mut self) {
        self.sort_by_key_pair(|e| (e.artist.to_lowercase(), e.title.to_lowercase()));
    }

    fn sort_by_key_pair(&mut self, key: impl Fn(&Entry) -> (String, String)) {
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_by_cached_key(|&i| key(&self.entries[i]));
        self.apply_order(&order);
    }

    /// Reverses the order (MISC ▸ Reverse).
    pub fn reverse(&mut self) {
        let order: Vec<usize> = (0..self.entries.len()).rev().collect();
        self.apply_order(&order);
    }

    /// Shuffles the order with Fisher-Yates (MISC ▸ Randomize). Pass a seeded `Rng` in tests.
    pub fn randomize(&mut self, rng: &mut fastrand::Rng) {
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        for i in (1..order.len()).rev() {
            let j = rng.usize(0..=i);
            order.swap(i, j);
        }
        self.apply_order(&order);
    }

    /// The row to play after the current one.
    ///
    /// * Shuffle: a random row other than the current one, whatever `repeat` says.
    /// * Repeat: the next row, wrapping from the last to the first.
    /// * Otherwise: the next row, or `None` after the last one.
    ///
    /// With nothing current the first row is next.
    pub fn next(&self, shuffle: bool, repeat: bool, rng: &mut fastrand::Rng) -> Option<usize> {
        self.step(1, shuffle, repeat, rng)
    }

    /// The row to play before the current one. Same rules as [`Playlist::next`], backwards.
    /// With nothing current the last row is previous when repeating, the first otherwise.
    pub fn prev(&self, shuffle: bool, repeat: bool, rng: &mut fastrand::Rng) -> Option<usize> {
        self.step(-1, shuffle, repeat, rng)
    }

    fn step(
        &self,
        dir: isize,
        shuffle: bool,
        repeat: bool,
        rng: &mut fastrand::Rng,
    ) -> Option<usize> {
        let n = self.entries.len();
        if n == 0 {
            return None;
        }
        if shuffle {
            return Some(random_other(n, self.current, rng));
        }
        let Some(current) = self.current.filter(|&c| c < n) else {
            return Some(if dir > 0 || !repeat { 0 } else { n - 1 });
        };
        let target = current as isize + dir;
        if repeat {
            Some(target.rem_euclid(n as isize) as usize)
        } else if target < 0 || target >= n as isize {
            None
        } else {
            Some(target as usize)
        }
    }

    /// The whole list as JSON: an array of `{video_id, title, artist, duration_secs}`.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.entries).expect("entries always serialize")
    }

    /// Parses a list written by [`Playlist::to_json`]. Nothing is current or selected.
    pub fn from_json(text: &str) -> Result<Playlist, serde_json::Error> {
        let entries: Vec<Entry> = serde_json::from_str(text)?;
        Ok(Playlist::from_entries(entries))
    }

    /// The list as text: one `https://music.youtube.com/watch?v=ID` link per line. Titles are not
    /// kept. [`parse_links`] reads the text back, which is how LIST ▸ SAVE writes a `.txt` list.
    pub fn to_links_text(&self) -> String {
        self.entries
            .iter()
            .map(|e| format!("https://music.youtube.com/watch?v={}\n", e.video_id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pl(n: usize) -> Playlist {
        let mut p = Playlist::default();
        for i in 0..n {
            p.add(Entry {
                video_id: format!("vid{i:07}"),
                title: format!("T{i}"),
                artist: String::new(),
                duration_secs: Some(60 * (i as u32 + 1)),
            });
        }
        p
    }

    fn ids(p: &Playlist) -> Vec<String> {
        p.entries().iter().map(|e| e.title.clone()).collect()
    }

    fn sel(p: &Playlist) -> Vec<usize> {
        p.selected().iter().copied().collect()
    }

    #[test]
    fn click_selects_one_row_and_sets_the_anchor() {
        let mut p = pl(5);
        p.click(2);
        assert_eq!(sel(&p), vec![2]);
        assert_eq!(p.anchor(), Some(2));
        p.click(9);
        assert_eq!(sel(&p), vec![2], "out of range clicks change nothing");
    }

    #[test]
    fn ctrl_click_toggles_and_moves_the_anchor() {
        let mut p = pl(5);
        p.click(1);
        p.ctrl_click(3);
        assert_eq!(sel(&p), vec![1, 3]);
        assert_eq!(p.anchor(), Some(3));
        p.ctrl_click(1);
        assert_eq!(
            sel(&p),
            vec![3],
            "ctrl-click on a selected row deselects it"
        );
        assert_eq!(
            p.anchor(),
            Some(1),
            "the anchor follows even a deselect, as Winamp 2"
        );
    }

    #[test]
    fn shift_click_selects_the_range_from_the_anchor_and_keeps_the_anchor() {
        let mut p = pl(8);
        p.shift_click(2);
        assert!(p.selected().is_empty(), "no anchor, so nothing happens");
        p.click(5);
        p.shift_click(2);
        assert_eq!(sel(&p), vec![2, 3, 4, 5]);
        assert_eq!(p.anchor(), Some(5));
        p.shift_click(7);
        assert_eq!(sel(&p), vec![5, 6, 7], "shift-click replaces the selection");
    }

    #[test]
    fn select_all_none_and_invert() {
        let mut p = pl(4);
        p.select_all();
        assert_eq!(sel(&p), vec![0, 1, 2, 3]);
        p.select_none();
        assert!(p.selected().is_empty());
        p.click(1);
        p.invert_selection();
        assert_eq!(sel(&p), vec![0, 2, 3]);
    }

    #[test]
    fn remove_selected_keeps_current_and_shifts_indices() {
        let mut p = pl(6);
        p.set_current(Some(4));
        p.click(1);
        p.ctrl_click(3);
        p.remove_selected();
        assert_eq!(ids(&p), vec!["T0", "T2", "T4", "T5"]);
        assert_eq!(
            p.current(),
            Some(2),
            "the current entry moved down with its entry"
        );
        assert!(p.selected().is_empty());
    }

    #[test]
    fn removing_the_current_entry_leaves_nothing_current() {
        let mut p = pl(3);
        p.set_current(Some(1));
        p.click(1);
        p.remove_selected();
        assert_eq!(p.current(), None);
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn crop_keeps_only_selected_and_selects_them_all() {
        let mut p = pl(5);
        p.set_current(Some(3));
        p.click(2);
        p.ctrl_click(3);
        p.crop();
        assert_eq!(ids(&p), vec!["T2", "T3"]);
        assert_eq!(sel(&p), vec![0, 1]);
        assert_eq!(p.current(), Some(1));

        let mut q = pl(3);
        q.set_current(Some(0));
        q.click(2);
        q.crop();
        assert_eq!(
            q.current(),
            None,
            "a current entry that was cropped away is gone"
        );
    }

    #[test]
    fn clear_empties_everything() {
        let mut p = pl(3);
        p.set_current(Some(0));
        p.click(0);
        p.clear();
        assert!(p.is_empty());
        assert_eq!(p.current(), None);
        assert!(p.selected().is_empty());
        assert_eq!(p.anchor(), None);
    }

    #[test]
    fn insert_shifts_later_rows_and_clamps_the_position() {
        let mut p = pl(3);
        p.set_current(Some(1));
        p.click(2);
        p.insert(0, Entry::from_id("new00000000"));
        assert_eq!(p.get(0).unwrap().video_id, "new00000000");
        assert_eq!(p.current(), Some(2));
        assert_eq!(sel(&p), vec![3]);
        p.insert(99, Entry::from_id("end00000000"));
        assert_eq!(p.len(), 5);
        assert_eq!(p.get(4).unwrap().video_id, "end00000000");
    }

    #[test]
    fn move_selected_moves_a_block_down_and_up() {
        let mut p = pl(6);
        p.set_current(Some(1));
        p.click(1);
        p.ctrl_click(2);
        p.move_selected(3);
        assert_eq!(ids(&p), vec!["T0", "T3", "T4", "T5", "T1", "T2"]);
        assert_eq!(sel(&p), vec![4, 5]);
        assert_eq!(p.current(), Some(4), "the current entry moves with its row");

        p.move_selected(-4);
        assert_eq!(ids(&p), vec!["T1", "T2", "T0", "T3", "T4", "T5"]);
        assert_eq!(sel(&p), vec![0, 1]);
    }

    #[test]
    fn move_selected_clamps_at_the_ends_and_ignores_zero() {
        let mut p = pl(4);
        p.click(2);
        p.move_selected(0);
        assert_eq!(ids(&p), vec!["T0", "T1", "T2", "T3"]);
        p.move_selected(10);
        assert_eq!(ids(&p), vec!["T0", "T1", "T3", "T2"]);
        assert_eq!(sel(&p), vec![3]);
        p.move_selected(-10);
        assert_eq!(ids(&p), vec!["T2", "T0", "T1", "T3"]);
        assert_eq!(sel(&p), vec![0]);
    }

    #[test]
    fn move_selected_gathers_separated_rows_into_one_block() {
        let mut p = pl(5);
        p.click(0);
        p.ctrl_click(4);
        p.move_selected(1);
        assert_eq!(ids(&p), vec!["T1", "T0", "T4", "T2", "T3"]);
        assert_eq!(sel(&p), vec![1, 2]);
    }

    #[test]
    fn sort_by_title_and_artist_are_case_insensitive_and_stable() {
        let mut p = Playlist::from_entries(vec![
            Entry {
                video_id: "a".into(),
                title: "beta".into(),
                artist: "Zed".into(),
                duration_secs: None,
            },
            Entry {
                video_id: "b".into(),
                title: "Alpha".into(),
                artist: "Yan".into(),
                duration_secs: None,
            },
            Entry {
                video_id: "c".into(),
                title: "alpha".into(),
                artist: "Xo".into(),
                duration_secs: None,
            },
        ]);
        p.set_current(Some(0));
        p.click(0);
        p.sort_by_title();
        let order: Vec<&str> = p.entries().iter().map(|e| e.video_id.as_str()).collect();
        assert_eq!(order, vec!["c", "b", "a"], "title first, then artist");
        assert_eq!(p.current(), Some(2), "current follows its entry");
        assert_eq!(sel(&p), vec![2]);

        p.sort_by_artist();
        let order: Vec<&str> = p.entries().iter().map(|e| e.video_id.as_str()).collect();
        assert_eq!(order, vec!["c", "b", "a"]);
    }

    #[test]
    fn reverse_flips_the_order_and_keeps_the_selection_on_its_entries() {
        let mut p = pl(4);
        p.set_current(Some(0));
        p.click(1);
        p.reverse();
        assert_eq!(ids(&p), vec!["T3", "T2", "T1", "T0"]);
        assert_eq!(p.current(), Some(3));
        assert_eq!(sel(&p), vec![2]);
    }

    #[test]
    fn randomize_is_a_permutation_and_repeats_for_a_seed() {
        let mut a = pl(20);
        let mut b = pl(20);
        a.randomize(&mut fastrand::Rng::with_seed(7));
        b.randomize(&mut fastrand::Rng::with_seed(7));
        assert_eq!(a, b);
        let mut sorted = ids(&a);
        sorted.sort();
        let mut expected: Vec<String> = (0..20).map(|i| format!("T{i}")).collect();
        expected.sort();
        assert_eq!(sorted, expected);
        assert_ne!(
            ids(&a),
            (0..20).map(|i| format!("T{i}")).collect::<Vec<_>>()
        );
    }

    #[test]
    fn totals_and_selected_time_skip_unknown_durations() {
        let mut p = pl(3);
        p.get_mut(1).unwrap().duration_secs = None;
        assert_eq!(p.total_secs(), 60 + 180);
        p.click(2);
        p.ctrl_click(1);
        assert_eq!(p.selected_secs(), 180);
    }

    #[test]
    fn next_without_shuffle_or_repeat_stops_at_the_end() {
        let p = {
            let mut p = pl(3);
            p.set_current(Some(1));
            p
        };
        let mut rng = fastrand::Rng::with_seed(1);
        assert_eq!(p.next(false, false, &mut rng), Some(2));
        assert_eq!(p.prev(false, false, &mut rng), Some(0));
        let last = {
            let mut q = pl(3);
            q.set_current(Some(2));
            q
        };
        assert_eq!(last.next(false, false, &mut rng), None);
        let first = {
            let mut q = pl(3);
            q.set_current(Some(0));
            q
        };
        assert_eq!(first.prev(false, false, &mut rng), None);
    }

    #[test]
    fn repeat_wraps_in_both_directions() {
        let mut rng = fastrand::Rng::with_seed(1);
        let mut p = pl(3);
        p.set_current(Some(2));
        assert_eq!(p.next(false, true, &mut rng), Some(0));
        p.set_current(Some(0));
        assert_eq!(p.prev(false, true, &mut rng), Some(2));
    }

    #[test]
    fn shuffle_never_picks_the_current_entry_unless_there_is_only_one() {
        let mut rng = fastrand::Rng::with_seed(99);
        let mut p = pl(4);
        p.set_current(Some(2));
        for _ in 0..500 {
            let next = p
                .next(true, false, &mut rng)
                .expect("shuffle always has a next");
            assert_ne!(next, 2);
            assert!(next < 4);
            let prev = p
                .prev(true, false, &mut rng)
                .expect("shuffle always has a prev");
            assert_ne!(prev, 2);
        }
        let mut seen = BTreeSet::new();
        for _ in 0..200 {
            seen.insert(p.next(true, false, &mut rng).unwrap());
        }
        assert_eq!(seen.len(), 3, "every other entry is reachable");

        let mut one = pl(1);
        one.set_current(Some(0));
        assert_eq!(one.next(true, false, &mut rng), Some(0));
    }

    #[test]
    fn with_nothing_current_next_is_the_first_row() {
        let mut rng = fastrand::Rng::with_seed(3);
        let p = pl(4);
        assert_eq!(p.next(false, false, &mut rng), Some(0));
        assert_eq!(p.prev(false, false, &mut rng), Some(0));
        assert_eq!(p.prev(false, true, &mut rng), Some(3));
        assert_eq!(Playlist::default().next(true, true, &mut rng), None);
    }

    #[test]
    fn display_lines_are_numbered_and_skip_a_missing_artist() {
        let mut p = Playlist::from_entries(vec![
            Entry {
                video_id: "a".into(),
                title: "Despacito".into(),
                artist: "Luis Fonsi".into(),
                duration_secs: Some(229),
            },
            Entry::from_id("kJQP7kiw5Fk"),
        ]);
        assert_eq!(
            p.display_line(0).as_deref(),
            Some("1. Luis Fonsi - Despacito")
        );
        assert_eq!(p.display_line(1).as_deref(), Some("2. kJQP7kiw5Fk"));
        assert_eq!(p.display_line(2), None);
        p.get_mut(1).unwrap().title = "  ".into();
        assert_eq!(p.display_line(1).as_deref(), Some("2. kJQP7kiw5Fk"));
    }

    #[test]
    fn index_of_finds_the_first_match() {
        let p = Playlist::from_entries(vec![
            Entry::from_id("aaaaaaaaaaa"),
            Entry::from_id("bbbbbbbbbbb"),
        ]);
        assert_eq!(p.index_of("bbbbbbbbbbb"), Some(1));
        assert_eq!(p.index_of("ccccccccccc"), None);
    }

    #[test]
    fn json_round_trips_the_entries_and_defaults_missing_fields() {
        let p = Playlist::from_entries(vec![Entry {
            video_id: "kJQP7kiw5Fk".into(),
            title: "Despacito".into(),
            artist: "Luis Fonsi".into(),
            duration_secs: Some(229),
        }]);
        let back = Playlist::from_json(&p.to_json()).expect("round trip");
        assert_eq!(back.entries(), p.entries());
        assert_eq!(back.current(), None);

        let minimal = Playlist::from_json(r#"[{"video_id":"kJQP7kiw5Fk"}]"#).expect("minimal");
        assert_eq!(minimal.get(0).unwrap().title, "");
        assert_eq!(minimal.get(0).unwrap().duration_secs, None);
        assert!(Playlist::from_json("{}").is_err());
    }

    #[test]
    fn links_text_has_one_link_per_line_and_reads_back_to_the_same_ids() {
        let p = Playlist::from_entries(vec![
            Entry::from_id("kJQP7kiw5Fk"),
            Entry::from_id("fJ9rUzIMcZQ"),
        ]);
        let text = p.to_links_text();
        assert_eq!(
            text,
            "https://music.youtube.com/watch?v=kJQP7kiw5Fk\n\
             https://music.youtube.com/watch?v=fJ9rUzIMcZQ\n"
        );
        let ids: Vec<Link> = parse_links(&text);
        assert_eq!(
            ids,
            vec![
                Link::Video("kJQP7kiw5Fk".into()),
                Link::Video("fJ9rUzIMcZQ".into()),
            ]
        );
        assert_eq!(Playlist::default().to_links_text(), "");
    }
}
