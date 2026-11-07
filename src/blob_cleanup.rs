use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct Scan {
    pub targets: Vec<(PathBuf, u64)>,
    pub occupied: Vec<PathBuf>,
    pub empty: Vec<PathBuf>,
    pub show_prompt: bool,
}

pub fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn dir_size(path: &Path) -> u64 {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

pub fn scan(
    roots: Vec<PathBuf>,
    current: &Path,
    known: &HashSet<String>,
    show_prompt: bool,
) -> Scan {
    let mut result = Scan {
        show_prompt,
        ..Default::default()
    };
    for root in roots {
        // A failed read (for example an offline drive) must not prune history.
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        let mut occupied = false;
        let mut uncertain = false;
        for entry in entries {
            let Ok(entry) = entry else {
                uncertain = true;
                continue;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(hash) = name.strip_prefix(".sendme-recv-") else {
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                uncertain = true;
                continue;
            };
            if !kind.is_dir() {
                continue;
            }
            occupied = true;
            // Remembered tickets only protect caches reachable at the current location.
            if !show_prompt || (root == current && known.contains(hash)) {
                continue;
            }
            let path = entry.path();
            let size = dir_size(&path);
            if size > 1024 * 1024 {
                result.targets.push((path, size));
            }
        }
        if occupied {
            result.occupied.push(root);
        } else if !uncertain {
            result.empty.push(root);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_destination_is_found_even_with_a_remembered_ticket() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("old");
        let current = temp.path().join("current");
        let hash = "a".repeat(64);
        for root in [&old, &current] {
            let cache = root.join(format!(".sendme-recv-{hash}"));
            fs::create_dir_all(&cache).unwrap();
            fs::write(cache.join("data"), vec![1; 1024 * 1024 + 1]).unwrap();
        }
        let result = scan(
            vec![old.clone(), current.clone()],
            &current,
            &HashSet::from([hash]),
            true,
        );
        assert_eq!(result.targets.len(), 1);
        assert_eq!(result.targets[0].0.parent(), Some(old.as_path()));
        assert_eq!(result.occupied, [old, current]);
    }

    #[test]
    fn only_readable_roots_without_blobs_are_pruned() {
        let temp = tempfile::tempdir().unwrap();
        let empty = temp.path().join("empty");
        let tiny = temp.path().join("tiny");
        let unavailable = temp.path().join("unavailable");
        fs::create_dir(&empty).unwrap();
        fs::write(empty.join("keep.txt"), b"downloaded file").unwrap();
        let cache = tiny.join(".sendme-recv-small");
        fs::create_dir_all(&cache).unwrap();
        let result = scan(
            vec![empty.clone(), tiny.clone(), unavailable.clone()],
            &empty,
            &HashSet::new(),
            true,
        );
        assert_eq!(result.empty, [empty.clone()]);
        assert_eq!(result.occupied, [tiny.clone()]);
        assert!(result.targets.is_empty());
        fs::create_dir(&unavailable).unwrap();
        let reconnected = scan(vec![unavailable.clone()], &empty, &HashSet::new(), false);
        assert_eq!(reconnected.empty, [unavailable]);
        fs::remove_dir(cache).unwrap();
        let result = scan(vec![tiny.clone()], &empty, &HashSet::new(), false);
        assert_eq!(result.empty, [tiny]);
        assert_eq!(
            fs::read(empty.join("keep.txt")).unwrap(),
            b"downloaded file"
        );
    }
}
