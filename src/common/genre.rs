use crate::utils::{GENRE_DELIM_AUTOMATON, GENRE_DELIM_EXCEPTION_AUTOMATON};
use aho_corasick::{AhoCorasick, Match};
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use std::cell::OnceCell;

/// Split a single Genre tag value into atomic genre names.
///
/// Mirrors `parse_mb_artist_tag`'s two-pass Aho-Corasick algorithm but consults
/// the genre automatons. **Differs deliberately** from the artist version: the
/// outer guard only requires the delimiter automaton to be present. An empty
/// exceptions list (the genre default) must NOT disable splitting.
pub fn parse_genre_tag(input: &str) -> Vec<&str> {
    let delim_guard = GENRE_DELIM_AUTOMATON.read().unwrap();
    let Some(delim_ac) = delim_guard.as_ref() else {
        return vec![input];
    };
    let exc_guard = GENRE_DELIM_EXCEPTION_AUTOMATON.read().unwrap();
    let exc_ac: Option<&AhoCorasick> = exc_guard.as_ref();

    let mut buffer: String = input.to_owned();
    let mut found: Vec<&str> = Vec::new();

    if let Some(exc_ac) = exc_ac {
        for mat in exc_ac.find_iter(input) {
            let start = mat.start();
            let end = mat.end();
            if let Some(name) = input.get(start..end) {
                found.push(name);
                let len = end - start;
                buffer.replace_range(start..end, &" ".repeat(len));
            }
        }
    }

    let matched_delims = delim_ac.find_iter(&buffer).collect::<Vec<Match>>();
    if matched_delims.is_empty() {
        if !found.is_empty() {
            return found;
        }
        return vec![input];
    }

    let first_range = 0..matched_delims[0].start();
    if buffer
        .get(first_range.clone())
        .is_some_and(|substr| !substr.trim().is_empty())
        && let Some(g) = input.get(first_range).map(str::trim)
    {
        found.push(g);
    }
    for i in 1..(matched_delims.len()) {
        let between = matched_delims[i - 1].end()..matched_delims[i].start();
        if buffer
            .get(between.clone())
            .is_some_and(|substr| !substr.trim().is_empty())
            && let Some(g) = input.get(between).map(str::trim)
        {
            found.push(g);
        }
    }
    let last_range = matched_delims.last().unwrap().end().min(buffer.len())..;
    if !buffer[last_range.clone()].trim().is_empty() {
        found.push(input[last_range].trim());
    }
    found
}

/// Apply the hybrid splitting rule to a song's full set of Genre tag values.
///
/// - `values.len() >= 2`: trust MPD; each value is its own genre, never re-split.
/// - `values.len() == 1`: pass through `parse_genre_tag`.
/// - `values.is_empty()`: returns empty vec.
///
/// Empty / whitespace-only entries are dropped. Output is owned `String`s
/// because callers store the result long-term.
pub fn parse_genre_values(values: &[String]) -> Vec<String> {
    if values.is_empty() {
        return Vec::new();
    }
    if values.len() >= 2 {
        return values
            .iter()
            .filter(|v| !v.trim().is_empty())
            .map(|v| v.trim().to_owned())
            .collect();
    }
    let single = values[0].as_str();
    if single.trim().is_empty() {
        return Vec::new();
    }
    parse_genre_tag(single)
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

mod imp {
    use super::*;
    use glib::{ParamSpec, ParamSpecString};
    use once_cell::sync::Lazy;

    #[derive(Default, Debug)]
    pub struct Genre {
        pub name: OnceCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Genre {
        const NAME: &'static str = "EuphonicaGenre";
        type Type = super::Genre;
    }

    impl ObjectImpl for Genre {
        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> =
                Lazy::new(|| vec![ParamSpecString::builder("name").read_only().build()]);
            PROPERTIES.as_ref()
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> glib::Value {
            match pspec.name() {
                "name" => self.obj().get_name().to_value(),
                _ => unimplemented!(),
            }
        }
    }
}

glib::wrapper! {
    pub struct Genre(ObjectSubclass<imp::Genre>);
}

impl Genre {
    pub fn new(name: &str) -> Self {
        let obj: Self = glib::Object::builder().build();
        let _ = obj.imp().name.set(name.to_owned());
        obj
    }

    pub fn get_name(&self) -> &str {
        self.imp().name.get().map(String::as_str).unwrap_or("")
    }
}

impl Default for Genre {
    fn default() -> Self {
        glib::Object::new()
    }
}

#[cfg(test)]
mod tests {
    //! These tests exercise the splitter against the default delimiters
    //! `[",", ";", "/"]` and an empty exceptions list. They do **not** rely on
    //! GSettings being initialised — instead they install fixed automatons
    //! into the static `RwLock`s, run the tested logic, and restore.

    use super::*;
    use crate::utils::{
        build_aho_corasick_automaton, GENRE_DELIM_AUTOMATON, GENRE_DELIM_EXCEPTION_AUTOMATON,
    };
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct AutomatonRestoreGuard {
        delim: Option<AhoCorasick>,
        excepts: Option<AhoCorasick>,
    }

    impl Drop for AutomatonRestoreGuard {
        fn drop(&mut self) {
            *GENRE_DELIM_AUTOMATON.write().unwrap() = self.delim.take();
            *GENRE_DELIM_EXCEPTION_AUTOMATON.write().unwrap() = self.excepts.take();
        }
    }

    fn with_automatons<F: FnOnce()>(delims: &[&str], excepts: &[&str], f: F) {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_delim = std::mem::replace(
            &mut *GENRE_DELIM_AUTOMATON.write().unwrap(),
            build_aho_corasick_automaton(delims),
        );
        let prev_excepts = std::mem::replace(
            &mut *GENRE_DELIM_EXCEPTION_AUTOMATON.write().unwrap(),
            build_aho_corasick_automaton(excepts),
        );
        let _restore = AutomatonRestoreGuard {
            delim: prev_delim,
            excepts: prev_excepts,
        };
        f();
    }

    #[test]
    fn single_simple_value_is_unchanged() {
        with_automatons(&[",", ";", "/"], &[], || {
            assert_eq!(
                parse_genre_values(&["Rock".to_owned()]),
                vec!["Rock".to_owned()]
            );
        });
    }

    #[test]
    fn single_compound_value_is_split_on_comma() {
        with_automatons(&[",", ";", "/"], &[], || {
            assert_eq!(
                parse_genre_values(&["Rock, Pop".to_owned()]),
                vec!["Rock".to_owned(), "Pop".to_owned()]
            );
        });
    }

    #[test]
    fn single_compound_value_is_split_on_semicolon() {
        with_automatons(&[",", ";", "/"], &[], || {
            assert_eq!(
                parse_genre_values(&["Rock; Pop; Jazz".to_owned()]),
                vec!["Rock".to_owned(), "Pop".to_owned(), "Jazz".to_owned()]
            );
        });
    }

    #[test]
    fn ampersand_is_not_a_default_delimiter() {
        with_automatons(&[",", ";", "/"], &[], || {
            assert_eq!(
                parse_genre_values(&["Drum & Bass".to_owned()]),
                vec!["Drum & Bass".to_owned()]
            );
        });
    }

    #[test]
    fn multi_value_response_is_trusted_not_resplit() {
        with_automatons(&[",", ";", "/"], &[], || {
            // Even though "Rock, Pop" contains a delimiter, MPD already gave us
            // a list — we must trust it.
            assert_eq!(
                parse_genre_values(&[
                    "Jazz".to_owned(),
                    "Rock, Pop".to_owned(),
                ]),
                vec!["Jazz".to_owned(), "Rock, Pop".to_owned()]
            );
        });
    }

    #[test]
    fn slash_exception_is_preserved() {
        with_automatons(&[",", ";", "/"], &["AC/DC"], || {
            assert_eq!(
                parse_genre_values(&["AC/DC".to_owned()]),
                vec!["AC/DC".to_owned()]
            );
        });
    }

    #[test]
    fn empty_exceptions_does_not_disable_splitting() {
        // This is the key regression test: with the artist version's outer
        // guard, an empty exceptions list returns the input unchanged. Genre
        // splitter must keep working.
        with_automatons(&[",", ";", "/"], &[], || {
            assert_eq!(
                parse_genre_values(&["Rock, Pop".to_owned()]),
                vec!["Rock".to_owned(), "Pop".to_owned()]
            );
        });
    }

    #[test]
    fn empty_input_returns_empty() {
        with_automatons(&[",", ";", "/"], &[], || {
            assert!(parse_genre_values(&[]).is_empty());
            assert!(parse_genre_values(&["".to_owned()]).is_empty());
            assert!(parse_genre_values(&["   ".to_owned()]).is_empty());
        });
    }

    #[test]
    fn whitespace_only_entries_are_dropped_in_multi() {
        with_automatons(&[",", ";", "/"], &[], || {
            assert_eq!(
                parse_genre_values(&["Rock".to_owned(), "  ".to_owned(), "Pop".to_owned()]),
                vec!["Rock".to_owned(), "Pop".to_owned()]
            );
        });
    }
}
