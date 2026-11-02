//! Shared hash-table types used across the crate.
//!
//! The scene loader and the renderer look up assets, effects, bind groups
//! and shader symbols by name in tight loops, so the standard library's
//! SipHash-based tables cost more than the work they index.  These aliases
//! use [`hashbrown`]'s SwissTable implementation with [`ahash`]'s fast,
//! non-cryptographic (but still DoS-resistant) hasher.
//!
//! [`ahash`]: https://docs.rs/ahash
//! [`hashbrown`]: https://docs.rs/hashbrown

/// The hasher backing [`Map`] and [`Set`].
///
/// `ahash` seeds itself from the operating system once per process, so the
/// tables stay resistant to hash flooding while hashing much faster than
/// the standard library's SipHash.
pub type Hasher = ahash::RandomState;

/// Hash map with `ahash`'s fast hasher.
pub type Map<K, V> = hashbrown::HashMap<K, V, Hasher>;

/// Hash set with `ahash`'s fast hasher.
pub type Set<T> = hashbrown::HashSet<T, Hasher>;

/// Create an empty [`Map`].
///
/// `hashbrown` only offers `new()` for its own default hasher, so maps with
/// a custom hasher have to be built through `with_hasher`.
pub fn new_map<K, V>() -> Map<K, V> {
    Map::with_hasher(Hasher::default())
}

/// Create an empty [`Map`] with room for `capacity` entries.
pub fn new_map_with_capacity<K, V>(capacity: usize) -> Map<K, V> {
    Map::with_capacity_and_hasher(capacity, Hasher::default())
}

/// Create an empty [`Set`].
pub fn new_set<T>() -> Set<T> {
    Set::with_hasher(Hasher::default())
}

/// Iterate a map's entries in a stable, key-sorted order.
///
/// Hash maps have no inherent order, so wherever the iteration order is
/// observable (generated shader source, cache keys, uniform declarations)
/// this keeps the output deterministic and identical to the previous
/// sorted-map behaviour.
pub fn sorted_entries<K, V>(map: &Map<K, V>) -> Vec<(&K, &V)>
where
    K: Ord,
{
    let mut entries: Vec<(&K, &V)> = map.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
}

/// Iterate a map's keys in a stable, sorted order.
pub fn sorted_keys<K, V>(map: &Map<K, V>) -> Vec<&K>
where
    K: Ord,
{
    let mut keys: Vec<&K> = map.keys().collect();
    keys.sort();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn sorted_iteration_matches_the_previous_sorted_map_order() {
        let mut map = new_map();
        for key in ["zeta", "alpha", "mid", "beta"] {
            map.insert(key.to_string(), key.len());
        }

        // The order the standard library's BTreeMap used to produce.
        let reference: Vec<&str> = BTreeMap::from([
            ("zeta", 4usize),
            ("alpha", 5),
            ("mid", 3),
            ("beta", 4),
        ])
        .keys()
        .copied()
        .collect();

        let sorted: Vec<&str> = sorted_entries(&map)
            .into_iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(sorted, reference);

        let keys: Vec<&str> = sorted_keys(&map)
            .into_iter()
            .map(|key| key.as_str())
            .collect();
        assert_eq!(keys, reference);
    }

    #[test]
    fn maps_hold_and_retrieve_entries() {
        let mut map = new_map_with_capacity(4);
        map.insert(String::from("a"), 1);
        map.insert(String::from("b"), 2);
        assert_eq!(map.get("a"), Some(&1));
        assert_eq!(map.len(), 2);

        let mut set = new_set();
        set.insert(String::from("x"));
        assert!(set.contains("x"));
    }
}
