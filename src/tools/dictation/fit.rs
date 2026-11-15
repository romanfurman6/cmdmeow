//! Fitting the words into the text around the caret: a space where one is
//! missing, a small first letter in the middle of a sentence and a capital
//! at the start of one, no period when the sentence goes on after the caret.
//! Only what's pasted changes, never the text around it.

/// The text just before and just after the caret (selection), as the
/// focused field reported it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Around {
    pub before: String,
    pub after: String,
}

/// Opening brackets and quotes: no space after them.
const OPENING: &[char] = &['(', '[', '{', '«', '„', '“', '‘', '¿', '¡'];
/// Closing brackets and quotes, skipped when looking for the sentence end.
const CLOSING: &[char] = &[')', ']', '}', '»', '”', '’', '"', '\''];
const SENTENCE_END: &[char] = &['.', '!', '?', '…'];

/// `text` fitted between `around.before` and `around.after`. `keep` are
/// words whose capital stays (the person's own words). Without `around`
/// (the app didn't say) the text goes in as it is.
pub fn fit(text: &str, around: Option<&Around>, keep: &[String]) -> String {
    let core = text.trim();
    let Some(a) = around else { return core.to_string() };
    if core.is_empty() {
        return String::new();
    }
    let left = a.before.trim_end_matches([' ', '\t', '\u{a0}']);
    let mut out = if starts_sentence(left) {
        capital(core)
    } else if mid_sentence(left) {
        small(core, keep)
    } else {
        core.to_string()
    };

    // the sentence goes on after the caret: no full stop of ours
    let next = a.after.split('\n').next().unwrap_or("").trim_start().chars().next();
    if let Some(n) = next {
        let one_period = out.ends_with('.') && !out.ends_with("..");
        if (one_period && (n.is_lowercase() || [',', ';', ':'].contains(&n))) || (SENTENCE_END.contains(&n) && out.ends_with(n)) {
            out.pop();
        }
    }

    let first = out.chars().next();
    let space_before = match a.before.chars().last() {
        None => false,
        Some(c) if c.is_whitespace() || OPENING.contains(&c) => false,
        // a straight quote opens when there's a space (or nothing) before it
        Some('"' | '\'') => !a.before.chars().rev().nth(1).is_none_or(char::is_whitespace),
        Some(_) => !first.is_some_and(|f| [',', '.', ';', ':', '!', '?', ')', ']', '}'].contains(&f)),
    };
    let space_after = a.after.chars().next().is_some_and(char::is_alphanumeric);
    format!("{}{out}{}", if space_before { " " } else { "" }, if space_after { " " } else { "" })
}

/// Nothing before, a new line, or a finished sentence (maybe in quotes).
fn starts_sentence(left: &str) -> bool {
    if left.is_empty() || left.ends_with('\n') || left.ends_with('\r') {
        return true;
    }
    let bare = left.trim_end_matches(CLOSING);
    bare.ends_with(['!', '?', '…']) || (bare.ends_with('.') && !bare.ends_with(".."))
}

/// A word, a comma or a dash right before: the sentence is still going.
fn mid_sentence(left: &str) -> bool {
    left.trim_end_matches(CLOSING).chars().last().is_some_and(|c| c.is_alphanumeric() || [',', ';', ':', '-', '–', '—'].contains(&c))
}

fn capital(s: &str) -> String {
    let mut cs = s.chars();
    match cs.next() {
        Some(f) if f.is_lowercase() => f.to_uppercase().chain(cs).collect(),
        _ => s.to_string(),
    }
}

/// The first letter made small, unless the first word looks like a name
/// that keeps its capital: "I", an acronym, a mixed-case word, or one of
/// `keep`.
fn small(s: &str, keep: &[String]) -> String {
    let word: String = s.chars().take_while(|c| c.is_alphanumeric() || *c == '\'' || *c == '’').collect();
    let mut wc = word.chars();
    let Some(f) = wc.next() else { return s.to_string() };
    let rest_small = wc.clone().all(|c| !c.is_uppercase());
    let is_i = word == "I" || word.starts_with("I'") || word.starts_with("I’");
    let kept = keep.iter().any(|k| k.split_whitespace().next() == Some(word.as_str()));
    if !f.is_uppercase() || !rest_small || is_i || kept {
        return s.to_string();
    }
    f.to_lowercase().chain(s.chars().skip(1)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(before: &str, after: &str) -> Around {
        Around { before: before.into(), after: after.into() }
    }

    #[test]
    fn without_the_text_around_nothing_changes() {
        assert_eq!(fit("Hello there.", None, &[]), "Hello there.");
        assert_eq!(fit("  ", Some(&at("a", "")), &[]), "");
    }

    #[test]
    fn a_space_goes_between_words() {
        assert_eq!(fit("World.", Some(&at("Hello.", "")), &[]), " World.");
        assert_eq!(fit("World.", Some(&at("Hello. ", "")), &[]), "World.");
        assert_eq!(fit("Word", Some(&at("(", ")")), &[]), "Word");
        assert_eq!(fit("Quoted", Some(&at("say \"", "")), &[]), "Quoted");
        assert_eq!(fit("Next.", Some(&at("He said \"no\"", "")), &[]), " next.");
    }

    #[test]
    fn mid_sentence_the_first_letter_is_small() {
        assert_eq!(fit("And then we left.", Some(&at("We ate,", "")), &[]), " and then we left.");
        assert_eq!(fit("Привіт усім.", Some(&at("Кажу", "")), &[]), " привіт усім.");
        // names and the like keep their capital
        assert_eq!(fit("I think so.", Some(&at("Well", "")), &[]), " I think so.");
        assert_eq!(fit("NASA said.", Some(&at("Then", "")), &[]), " NASA said.");
        assert_eq!(fit("iPhone works.", Some(&at("My", "")), &[]), " iPhone works.");
        assert_eq!(fit("Claude said.", Some(&at("Then", "")), &["Claude".into()]), " Claude said.");
    }

    #[test]
    fn a_new_sentence_starts_with_a_capital() {
        assert_eq!(fit("then we left.", Some(&at("We ate.", "")), &[]), " Then we left.");
        assert_eq!(fit("then we left.", Some(&at("Done!\n", "")), &[]), "Then we left.");
        assert_eq!(fit("then.", Some(&at("", "")), &[]), "Then.");
        assert_eq!(fit("ok.", Some(&at("He said \"stop.\"", "")), &[]), " Ok.");
    }

    #[test]
    fn no_full_stop_when_the_sentence_goes_on() {
        assert_eq!(fit("There.", Some(&at("Hello ", "world")), &[]), "there ");
        assert_eq!(fit("Fine.", Some(&at("All ", ", thanks")), &[]), "fine");
        assert_eq!(fit("Really?", Some(&at("", "? yes")), &[]), "Really");
        // a new sentence after the caret: keep it
        assert_eq!(fit("Done.", Some(&at("", " Next one")), &[]), "Done.");
        // an ellipsis is meant
        assert_eq!(fit("Wait...", Some(&at("", "and")), &[]), "Wait... ");
    }
}
