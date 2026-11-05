//! Source shuffle order: retain the complete physical list and start at current.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
pub(super) fn order(
    original: &[PathBuf],
    current: Option<usize>,
    albums: bool,
    key: impl Fn(&Path) -> String,
    rng: &mut u64,
) -> Vec<PathBuf> {
    if original.is_empty() {
        return Vec::new();
    }
    let current = current.filter(|&i| i < original.len());
    let mut roll = |len: usize| {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        *rng as usize % len
    };
    if !albums {
        let mut indices = (0..original.len())
            .filter(|&i| Some(i) != current)
            .collect::<Vec<_>>();
        for i in (1..indices.len()).rev() {
            let j = roll(i + 1);
            indices.swap(i, j);
        }
        let mut out = Vec::with_capacity(original.len());
        if let Some(current) = current {
            out.push(original[current].clone());
        }
        out.extend(indices.into_iter().map(|i| original[i].clone()));
        return out;
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for (i, path) in original.iter().enumerate() {
        let key = key(path);
        let group = *by_key.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group].push(i);
    }
    let mut first = Vec::new();
    if let Some(current) = current {
        let selected = groups.iter().position(|g| g.contains(&current)).unwrap();
        first = groups.remove(selected);
        let pos = first.iter().position(|&i| i == current).unwrap();
        first.rotate_left(pos);
    }
    for i in (1..groups.len()).rev() {
        let j = roll(i + 1);
        groups.swap(i, j);
    }
    let mut indices = first;
    for group in groups {
        indices.extend(group);
    }
    indices.into_iter().map(|i| original[i].clone()).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn songs_from_middle_include_the_complete_physical_list() {
        let original = (0..10)
            .map(|i| PathBuf::from(format!("/music/{i}.flac")))
            .collect::<Vec<_>>();
        let mut rng = 123;
        let out = order(&original, Some(5), false, |_| String::new(), &mut rng);
        assert_eq!(out[0], original[5]);
        let mut sorted = out;
        sorted.sort();
        assert_eq!(sorted, original);
    }
    #[test]
    fn albums_keep_separate_folders_and_rotate_the_current_album() {
        let paths = [
            "/music/a/1",
            "/music/a/2",
            "/music/b/1",
            "/music/b/2",
            "/music/c/1",
        ]
        .map(PathBuf::from);
        let out = order(
            &paths,
            Some(1),
            true,
            |p| p.parent().unwrap().display().to_string(),
            &mut 91,
        );
        assert_eq!(&out[..2], &[paths[1].clone(), paths[0].clone()]);
        let b = out.iter().position(|p| p == &paths[2]).unwrap();
        assert_eq!(out[b + 1], paths[3]);
        let mut a = out;
        let mut b = paths.to_vec();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }
}
