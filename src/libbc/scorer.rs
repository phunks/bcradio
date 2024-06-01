use itertools::Itertools;
use rust_fuzzy_search::fuzzy_compare;
use crate::models::shared_data_models::Track;

#[derive(Clone)]
struct Score<'a, I> {
    iter: I,
    key: &'a str,
}

impl<I> Iterator for Score<'_, I>
    where I: Iterator<Item = Track>
{
    type Item = (f32, I::Item);

    #[inline]
    fn next(&mut self) -> Option<(f32, I::Item)> {
        #[allow(unused_assignments)]
        let mut score = 0_f32;
        match self.iter.next() {
            None => None,
            Some(c) => {
                score = fuzzy_compare(self.key, c.artist_name.to_lowercase().as_str());
                if score == 0_f32 {
                    score = fuzzy_compare(self.key, c.track.to_lowercase().as_str()) * 0.9;
                }
                Some((score, c))
            },
        }
    }
}

trait Scorer {
    fn scorer(self, s: &str) -> Score<Self> where Self: Sized;
}

impl<I> Scorer for I where I: Iterator, I: Sized {
    #[inline]
    fn scorer(self, s: &str) -> Score<Self> where Self: Sized {
        Score { iter: self, key: s }
    }
}

pub fn score_sort(v: Vec<Track>, s: &str) -> Vec<Track> {
    let mut aa = v.into_iter().scorer(s.to_lowercase().as_str()).collect_vec();
    aa.sort_by(|(a,_), (b,_)| a.partial_cmp(b).unwrap());
    let (_, d): (Vec<f32>, Vec<Track>) = aa.into_iter().unzip();
    d
}
