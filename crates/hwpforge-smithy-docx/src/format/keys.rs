use super::*;
use std::{
    collections::{hash_map::RandomState, HashMap, HashSet},
    hash::BuildHasher,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct FontKey(pub(super) usize);
#[derive(Default)]
pub(super) struct Fonts {
    names: Vec<Rc<str>>,
    buckets: HashMap<u64, Vec<FontKey>>,
    hasher: RandomState,
    source_fonts: HashSet<FontKey>,
    #[cfg(test)]
    pub(super) hash_bytes: usize,
    #[cfg(test)]
    pub(super) comparison_bytes: usize,
    #[cfg(test)]
    pub(super) copied_bytes: usize,
}
impl Fonts {
    pub(super) fn intern(&mut self, name: &str, part: &str, at: &str) -> Result<FontKey> {
        self.insert(name, part, at, true)
    }
    pub(super) fn fallback(&mut self) -> Result<FontKey> {
        self.insert("함초롬바탕", "#format", "font", false)
    }
    fn admit_source(&mut self, key: FontKey, part: &str, at: &str) -> Result<()> {
        if !self.source_fonts.contains(&key) {
            limit(
                part,
                at,
                "source font definitions",
                self.source_fonts.len() + 1,
                MAX_DEFINITIONS,
            )?;
            self.source_fonts.insert(key);
        }
        Ok(())
    }
    fn insert(&mut self, name: &str, part: &str, at: &str, source: bool) -> Result<FontKey> {
        crate::identifier::check(name, part, at)?;
        #[cfg(test)]
        {
            self.hash_bytes += name.len();
        }
        let hash = self.hasher.hash_one(name);
        if let Some(bucket) = self.buckets.get(&hash) {
            for &id in bucket {
                #[cfg(test)]
                {
                    self.comparison_bytes += self.names[id.0].len().min(name.len());
                }
                if self.names[id.0].as_ref() == name {
                    if source {
                        self.admit_source(id, part, at)?;
                    }
                    return Ok(id);
                }
            }
        }
        let id = FontKey(self.names.len());
        if source {
            self.admit_source(id, part, at)?;
        }
        #[cfg(test)]
        {
            self.copied_bytes += name.len();
        }
        self.names.push(Rc::from(name));
        self.buckets.entry(hash).or_default().push(id);
        Ok(id)
    }
    pub(super) fn name(&self, key: FontKey) -> &str {
        &self.names[key.0]
    }
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
