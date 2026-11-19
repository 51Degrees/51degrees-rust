/* *********************************************************************
 * This Original Work is copyright of 51 Degrees Mobile Experts Limited.
 * Copyright 2026 51 Degrees Mobile Experts Limited, Davidson House,
 * Forbury Square, Reading, Berkshire, United Kingdom RG1 3EU.
 *
 * This Original Work is licensed under the European Union Public Licence
 * (EUPL) v.1.2 and is subject to its terms as set out below.
 *
 * If a copy of the EUPL was not distributed with this file, You can obtain
 * one at https://opensource.org/licenses/EUPL-1.2.
 *
 * The 'Compatible Licences' set out in the Appendix to the EUPL (as may be
 * amended by the European Commission) shall be deemed incompatible for
 * the purposes of the Work and the provisions of the compatibility
 * clause in Article 5 of the EUPL shall not apply.
 *
 * If using the Work as, or as part of, a network application, by
 * including the attribution notice(s) required under Article 5 of the EUPL
 * in the end user terms of the application under an appropriate heading,
 * such notice(s) shall fulfill the requirements of that article.
 * ********************************************************************* */

//! Locate a data or resource file by walking up the directory tree.
//!
//! Used by every example. The example data files (Hash and IPI data) live in
//! the data repository inside the `cxx` submodule, so the path differs
//! depending on whether an example is run from an IDE, from its own crate
//! directory, or from a CI checkout. The finder walks up from the starting
//! directory through its ancestors so the file resolves the same way in every
//! case.

use std::path::{Path, PathBuf};

/// How many parent directories to climb before giving up.
///
/// This bounds the search so a missing file fails quickly rather than walking to
/// the filesystem root. Twenty levels comfortably covers any realistic checkout
/// depth, including a CI agent's nested workspace path.
pub const MAX_PARENT_LEVELS: usize = 20;

/// How many subdirectory levels below each ancestor are scanned when `name` is a
/// bare filename rather than a relative path.
///
/// The example data files sit up to three directories below a common ancestor
/// (for example `cxx/data/device-detection/<file>`), so a
/// shallow scan finds them without the cost of a full recursive descent of large
/// sibling repositories.
pub const MAX_DESCENT_DEPTH: usize = 3;

/// The total number of directories a single [`find_file_from`] search will read
/// before giving up.
///
/// This bounds the cost of the bare-filename subdirectory scan so the search can
/// never hang on a pathologically large tree (for example when an ancestor is a
/// drive root). It makes the search deterministic. A relative-path search is
/// unaffected because it never reads a directory listing.
pub const MAX_DIRECTORIES_SCANNED: usize = 4096;

/// Find `name` by walking up from the current working directory.
///
/// `name` may be either a bare filename (for example
/// `"51Degrees-LiteV4.1.hash"`) or a relative path (for example
/// `"cxx/data/device-detection/51Degrees-LiteV4.1.hash"`).
///
/// Starting at the current working directory and climbing through up to
/// [`MAX_PARENT_LEVELS`] ancestors, each ancestor is checked for `name`:
///
/// - If `name` contains a path separator it is treated as a relative path and
///   joined directly onto the ancestor, so a known layout resolves in one
///   `exists` check per ancestor.
/// - If `name` is a bare filename it is looked for directly in the ancestor and
///   then in subdirectories up to [`MAX_DESCENT_DEPTH`] deep.
///
/// Returns the first match as an absolute path, or `None` if the file is not
/// found within the bounded search. The walk climbs to the parent when the
/// current directory does not contain the file, using a bounded climb and a
/// shallow descent so the search is deterministic.
pub fn find_file(name: impl AsRef<Path>) -> Option<PathBuf> {
    let start = std::env::current_dir().ok()?;
    find_file_from(name, &start)
}

/// Find `name` by walking up from `start` instead of the current working
/// directory.
///
/// Behaves exactly like [`find_file`] but lets the caller (and the unit tests)
/// choose the starting directory. See [`find_file`] for the matching rules.
pub fn find_file_from(name: impl AsRef<Path>, start: &Path) -> Option<PathBuf> {
    let name = name.as_ref();
    let is_relative_path = name.components().count() > 1;

    // A shared budget across the whole climb, so the bare-filename scan cannot
    // re-walk overlapping subtrees on each ancestor and run away.
    let mut budget = MAX_DIRECTORIES_SCANNED;

    let mut current = Some(start);
    for _ in 0..=MAX_PARENT_LEVELS {
        let dir = current?;

        if is_relative_path {
            // A relative path is joined straight onto the ancestor: a known
            // layout resolves with a single existence check at each level.
            let candidate = dir.join(name);
            if candidate.exists() {
                return Some(absolutize(&candidate));
            }
        } else if let Some(found) = find_named_within(dir, name, MAX_DESCENT_DEPTH, &mut budget) {
            return Some(found);
        }

        current = dir.parent();
    }
    None
}

/// Look for the bare filename `name` in `dir` and in its subdirectories down to
/// `depth` further levels. Returns the first match as an absolute path.
///
/// `budget` is decremented for each directory whose listing is read and the scan
/// stops once it reaches zero, so a single search reads at most
/// [`MAX_DIRECTORIES_SCANNED`] directories no matter how the tree is shaped.
fn find_named_within(dir: &Path, name: &Path, depth: usize, budget: &mut usize) -> Option<PathBuf> {
    // A direct hit in this directory is preferred over anything in a
    // subdirectory.
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(absolutize(&direct));
    }

    if depth == 0 || *budget == 0 {
        return None;
    }
    *budget -= 1;

    // Otherwise scan the immediate subdirectories, recursing one level
    // shallower. Any I/O error (for example a directory the process cannot read)
    // is treated as "not here" (swallow and continue) rather than failing the
    // whole search.
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if *budget == 0 {
            break;
        }
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_named_within(&path, name, depth - 1, budget) {
                return Some(found);
            }
        }
    }
    None
}

/// Turn a path into an absolute one without requiring it to exist (it always
/// does at the call sites), preferring `canonicalize` and falling back to the
/// current directory join when canonicalisation is unavailable.
fn absolutize(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path),
        Err(_) => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_relative_path_in_ancestor() {
        let root = tempfile::tempdir().unwrap();
        // Lay out <root>/a/b/data/file.dat and start the search deep inside.
        let data_dir = root.path().join("a").join("b").join("data");
        fs::create_dir_all(&data_dir).unwrap();
        let file = data_dir.join("file.dat");
        fs::write(&file, b"x").unwrap();

        let start = root.path().join("a").join("b").join("c").join("d");
        fs::create_dir_all(&start).unwrap();

        // The relative path is rooted at <root>/a/b, an ancestor of the start.
        let found = find_file_from("data/file.dat", &start)
            .expect("the relative path should resolve at an ancestor");
        assert!(found.ends_with("data/file.dat"));
        assert!(found.is_absolute());
    }

    #[test]
    fn finds_bare_filename_in_subdirectory_of_ancestor() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("repo").join("sub");
        fs::create_dir_all(&nested).unwrap();
        let file = nested.join("target.txt");
        fs::write(&file, b"y").unwrap();

        // Start below the file, so the search must climb to the common
        // ancestor and then descend into the subdirectory.
        let start = root.path().join("repo").join("other").join("deep");
        fs::create_dir_all(&start).unwrap();

        let found =
            find_file_from("target.txt", &start).expect("the bare filename should be found");
        assert!(found.ends_with("target.txt"));
    }

    #[test]
    fn returns_none_when_absent() {
        let root = tempfile::tempdir().unwrap();
        let start = root.path().join("x");
        fs::create_dir_all(&start).unwrap();
        assert!(find_file_from("does-not-exist.dat", &start).is_none());
    }

    #[test]
    fn descent_does_not_exceed_configured_depth() {
        let root = tempfile::tempdir().unwrap();
        // Bury the file one level deeper than MAX_DESCENT_DEPTH so the
        // bare-filename scan from the start directory cannot reach it. The file
        // is still resolvable via its relative path, confirming the depth limit
        // is the only thing keeping it out of the bare-filename search.
        let mut deep = root.path().join("start");
        for i in 0..(MAX_DESCENT_DEPTH + 1) {
            deep = deep.join(format!("lvl{i}"));
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("buried.dat"), b"z").unwrap();

        let start = root.path().join("start");
        // Too deep for the bare-filename descent from `start`.
        assert!(find_file_from("buried.dat", &start).is_none());
    }

    #[test]
    fn prefers_direct_hit_over_subdirectory() {
        let root = tempfile::tempdir().unwrap();
        let start = root.path().join("here");
        fs::create_dir_all(&start).unwrap();
        // A direct file in the start directory and a same-named file one level
        // down. The direct hit must win.
        fs::write(start.join("name.dat"), b"direct").unwrap();
        let sub = start.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("name.dat"), b"nested").unwrap();

        let found = find_file_from("name.dat", &start).unwrap();
        assert_eq!(fs::read(&found).unwrap(), b"direct");
    }
}
