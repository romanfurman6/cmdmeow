//! History search: every word as typed first; when nothing matches that way,
//! a forgiving match where a word's letters only have to appear in order and
//! close together ("gthb" finds "github"). Also finds what to highlight in a
//! row's title.

/// Characters of a clip a forgiving match looks at (exact matching reads all).
const FUZZY_CHARS: usize = 5_000;
/// Words shorter than this only match as typed: two loose letters match nearly anything.
const FUZZY_MIN: usize = 3;
/// A word's letters may spread over at most this many times its length.
const SPREAD: usize = 3;

/// Lower case, one char for one char, so positions in the folded text are
/// positions in the original.
pub fn fold(s: &str) -> Vec<char> {
    s.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect()
}

/// The search box, split into folded words.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub words: Vec<Vec<char>>,
}

impl Query {
    pub fn new(q: &str) -> Query {
        Query { words: q.split_whitespace().map(fold).collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
}

pub fn find(hay: &[char], word: &[char], from: usize) -> Option<usize> {
    if word.is_empty() || hay.len() < word.len() {
        return None;
    }
    (from..=hay.len() - word.len()).find(|&i| hay[i..i + word.len()] == *word)
}

/// Every word appears as typed.
pub fn exact(hay: &[char], q: &Query) -> bool {
    q.words.iter().all(|w| find(hay, w, 0).is_some())
}

fn boundary(hay: &[char], i: usize) -> bool {
    i == 0 || !hay[i - 1].is_alphanumeric()
}

/// The tightest in-order placement of `word`'s letters in `hay`: the
/// positions and a score (higher is better), or None.
fn place(hay: &[char], word: &[char]) -> Option<(Vec<usize>, i32)> {
    let first = *word.first()?;
    let mut best: Option<(Vec<usize>, i32)> = None;
    for start in (0..hay.len()).filter(|&i| hay[i] == first) {
        let mut pos = Vec::with_capacity(word.len());
        pos.push(start);
        let mut j = start + 1;
        for &c in &word[1..] {
            match hay[j..].iter().position(|&h| h == c) {
                Some(k) => {
                    pos.push(j + k);
                    j += k + 1;
                }
                None => break,
            }
        }
        if pos.len() < word.len() {
            // later starts can't do better: the letters ran out
            break;
        }
        let span = pos[pos.len() - 1] - start + 1;
        if span > word.len() * SPREAD {
            continue;
        }
        let gaps = (span - word.len()) as i32;
        let runs = pos.windows(2).filter(|p| p[1] == p[0] + 1).count() as i32;
        let score = 100 - gaps * 4 + runs * 3 + if boundary(hay, start) { 10 } else { 0 };
        if best.as_ref().is_none_or(|(_, s)| score > *s) {
            best = Some((pos, score));
        }
    }
    best
}

/// A forgiving match of every word: its score, or None.
pub fn fuzzy(hay: &[char], q: &Query) -> Option<i32> {
    let hay = &hay[..hay.len().min(FUZZY_CHARS)];
    let mut total = 0;
    for w in &q.words {
        if find(hay, w, 0).is_some() {
            total += 150;
            continue;
        }
        if w.len() < FUZZY_MIN {
            return None;
        }
        total += place(hay, w)?.1;
    }
    Some(total)
}

/// Character ranges of `title` to highlight for `q` (start, end), sorted and merged.
pub fn marks(title: &str, q: &Query) -> Vec<(usize, usize)> {
    let hay = fold(title);
    let mut out: Vec<(usize, usize)> = Vec::new();
    for w in &q.words {
        let mut from = 0;
        let mut any = false;
        while let Some(i) = find(&hay, w, from) {
            out.push((i, i + w.len()));
            from = i + w.len();
            any = true;
        }
        if !any && w.len() >= FUZZY_MIN {
            if let Some((pos, _)) = place(&hay, w) {
                out.extend(pos.into_iter().map(|p| (p, p + 1)));
            }
        }
    }
    out.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (a, b) in out {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hay(s: &str) -> Vec<char> {
        fold(s)
    }

    #[test]
    fn exact_words_any_order() {
        let h = hay("Ship the Clipboard widget before Friday");
        assert!(exact(&h, &Query::new("friday clip")));
        assert!(!exact(&h, &Query::new("friday monday")));
    }

    #[test]
    fn fuzzy_finds_letters_in_order() {
        let h = hay("https://github.com/romanfurman6/cmdmeow");
        assert!(fuzzy(&h, &Query::new("gthb")).is_some());
        assert!(fuzzy(&h, &Query::new("cmdmw")).is_some());
        assert!(fuzzy(&h, &Query::new("bhtg")).is_none(), "order matters");
        assert!(fuzzy(&h, &Query::new("gw")).is_none(), "short words only as typed");
    }

    #[test]
    fn fuzzy_rejects_far_flung_letters() {
        // g…i…t spread over a long sentence is not "git"
        let h = hay("going to visit the other town");
        assert!(fuzzy(&h, &Query::new("gtt")).is_none());
    }

    #[test]
    fn tighter_matches_score_higher() {
        let q = Query::new("clip");
        let tight = fuzzy(&hay("clipboard"), &q).unwrap();
        let loose = fuzzy(&hay("c-l-i-p"), &q).unwrap();
        assert!(tight > loose);
        let qq = Query::new("cpbd");
        let start = fuzzy(&hay("clipboard"), &qq).unwrap();
        let mid = fuzzy(&hay("xclipboard"), &qq).unwrap();
        assert!(start > mid, "a word start counts");
    }

    #[test]
    fn non_ascii_positions_line_up() {
        let t = "Привіт, Світ";
        let m = marks(t, &Query::new("світ"));
        // "Привіт" doesn't hold "світ"; the second word does, at 8..12
        assert_eq!(m, vec![(8, 12)]);
        let chars: Vec<char> = t.chars().collect();
        assert_eq!(chars[8..12].iter().collect::<String>(), "Світ");
    }

    #[test]
    fn marks_merge_and_fall_back_to_fuzzy() {
        assert_eq!(marks("aaa", &Query::new("aa")), vec![(0, 2)]);
        assert_eq!(marks("Clipboard clip", &Query::new("clip board")), vec![(0, 9), (10, 14)]);
        assert_eq!(marks("github", &Query::new("gthb")), vec![(0, 1), (2, 4), (5, 6)]);
        assert!(marks("nothing", &Query::new("zzz")).is_empty());
    }
}
