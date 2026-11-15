//! The person's own words (names, brands, terms): what the model heard as
//! something that sounds like one of them becomes that word, spelled their
//! way. "клод" or "Clode" → "Claude", "cmd meow" → "CmdMeow".
//!
//! Each word gets a rough sound key: lower case, Cyrillic spelled in Latin,
//! letters that sound alike merged. One to three heard words are compared
//! with it: short words must sound the same, longer ones may differ by a
//! letter or so.

/// Longest run of heard words compared with one of the person's words.
const MAX_SPAN: usize = 3;

/// One of the person's words with its sound key.
struct Known<'a> {
    word: &'a str,
    key: String,
    /// How many words it is ("Cmd Meow" is 2).
    parts: usize,
}

/// `text` with heard words replaced by the person's `words`.
pub fn apply(text: &str, words: &[String]) -> String {
    let known: Vec<Known> = words
        .iter()
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .map(|w| Known { word: w, key: sound_key(w), parts: w.split_whitespace().count() })
        .filter(|k| k.key.chars().count() >= 3)
        .collect();
    if known.is_empty() {
        return text.to_string();
    }
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    'next: while i < tokens.len() {
        for n in (1..=MAX_SPAN.min(tokens.len() - i)).rev() {
            let span = &tokens[i..i + n];
            let joined = span.join(" ");
            let (lead, core, trail) = split_punct(&joined);
            // punctuation inside a span means two thoughts, not one word
            if core.is_empty() || core.chars().any(|c| !c.is_alphanumeric() && !c.is_whitespace() && c != '-' && c != '\'' && c != '’') {
                continue;
            }
            let key = sound_key(core);
            // the same number of words may differ a little; one more heard
            // word ("cmd meow" for CmdMeow) must sound exactly the same
            if let Some(k) = known.iter().find(|k| (n == k.parts && sounds_like(&key, &k.key)) || (n == k.parts + 1 && key == k.key)) {
                out.push(format!("{lead}{}{trail}", k.word));
                i += n;
                continue 'next;
            }
        }
        out.push(tokens[i].to_string());
        i += 1;
    }
    out.join(" ")
}

/// Leading punctuation, the word part, trailing punctuation.
fn split_punct(s: &str) -> (&str, &str, &str) {
    let start = s.find(|c: char| c.is_alphanumeric()).unwrap_or(s.len());
    let end = s.rfind(|c: char| c.is_alphanumeric()).map(|k| k + s[k..].chars().next().map_or(1, char::len_utf8)).unwrap_or(start);
    (&s[..start], &s[start..end], &s[end..])
}

fn sounds_like(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (la, lb) = (a.chars().count(), b.chars().count());
    let longest = la.max(lb);
    // short keys: the same sound or nothing ("klod" must not take "kold",
    // "roman" not "romans")
    if la.min(lb) < 6 {
        return false;
    }
    let allowed = if longest >= 9 { 2 } else { 1 };
    la.abs_diff(lb) <= allowed && distance(a, b) <= allowed
}

/// Edits (insert, delete, change one letter) from `a` to `b`.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let cur = row[j + 1];
            row[j + 1] = if ca == b[j] { prev } else { 1 + prev.min(cur).min(row[j]) };
            prev = cur;
        }
    }
    row[b.len()]
}

/// A rough key for how `s` sounds; equal keys mostly mean "said the same".
pub fn sound_key(s: &str) -> String {
    let mut latin = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            latin.push(c);
        } else if let Some(l) = cyrillic(c) {
            latin.push_str(l);
        }
        // spaces, hyphens, apostrophes and the rest: not sounds
    }
    let mut k = latin;
    for (from, to) in [("dzh", "j"), ("sch", "sh"), ("tch", "ch"), ("ck", "k"), ("ph", "f"), ("th", "t"), ("kh", "h"), ("au", "o"), ("aw", "o"), ("oo", "u"), ("ee", "i"), ("ea", "i"), ("ou", "au")] {
        k = k.replace(from, to);
    }
    let mut out = String::with_capacity(k.len());
    let mut chars = k.chars().peekable();
    while let Some(c) = chars.next() {
        let c = match c {
            // "c" before e/i sounds like s, else like k
            'c' if chars.peek().is_some_and(|n| matches!(n, 'e' | 'i' | 'y')) => 's',
            'c' | 'q' => 'k',
            'w' => 'v',
            'y' => 'i',
            'x' => {
                out.push('k');
                's'
            }
            c => c,
        };
        // doubled letters sound like one
        if out.ends_with(c) {
            continue;
        }
        out.push(c);
    }
    // a final silent "e" (Claude, Google)
    if out.len() > 3 && out.ends_with('e') {
        out.pop();
    }
    out
}

/// Ukrainian and Russian letters as they sound in Latin.
fn cyrillic(c: char) -> Option<&'static str> {
    Some(match c {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' | 'ґ' => "g",
        'д' => "d",
        'е' | 'э' => "e",
        'є' => "ye",
        'ж' => "zh",
        'з' => "z",
        'и' | 'і' | 'ї' | 'ы' | 'й' => "i",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "h",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "shch",
        'ь' | 'ъ' => "",
        'ю' => "yu",
        'я' => "ya",
        'ё' => "yo",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(w: &[&str]) -> Vec<String> {
        w.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn names_heard_in_cyrillic_or_misspelled_are_fixed() {
        let w = words(&["Claude", "CmdMeow", "Parakeet"]);
        assert_eq!(apply("Спитай клод про це.", &w), "Спитай Claude про це.");
        assert_eq!(apply("ask Clode, please", &w), "ask Claude, please");
        assert_eq!(apply("open cmd meow now", &w), "open CmdMeow now");
        assert_eq!(apply("the Parakit model", &w), "the Parakeet model");
        assert_eq!(apply("(cmdmeow)", &w), "(CmdMeow)");
    }

    #[test]
    fn ordinary_words_are_left_alone() {
        let w = words(&["Claude", "Roman", "Parakeet"]);
        let s = "I put it in the cloud, it was cold and loud. Romance novels, the parrot kit";
        assert_eq!(apply(s, &w), s);
    }

    #[test]
    fn no_words_or_blank_words_change_nothing() {
        assert_eq!(apply("hello  world", &[]), "hello  world");
        assert_eq!(apply("hello world", &words(&["  ", "ab"])), "hello world");
    }

    #[test]
    fn sentence_punctuation_is_not_merged_away() {
        let w = words(&["CmdMeow"]);
        // "cmd." ends a sentence: "cmd. meow" is two things
        assert_eq!(apply("type cmd. meow", &w), "type cmd. meow");
    }

    #[test]
    fn sound_keys() {
        assert_eq!(sound_key("Claude"), "klod");
        assert_eq!(sound_key("Клод"), "klod");
        assert_eq!(sound_key("Google"), "gugl");
        assert_eq!(sound_key("Гугл"), "gugl");
        assert_eq!(sound_key("Cmd Meow"), sound_key("cmdmeow"));
        assert_ne!(sound_key("cloud"), sound_key("Claude"));
    }

    #[test]
    fn edit_distance() {
        assert_eq!(distance("kitten", "sitting"), 3);
        assert_eq!(distance("", "abc"), 3);
        assert_eq!(distance("клод", "клод"), 0);
    }
}
