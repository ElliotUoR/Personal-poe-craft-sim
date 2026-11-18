//! Exhaustive enumeration of every random outcome of a function that takes a `Chooser`.
//!
//! The function is re-run once per outcome path; each run replays a prefix of choices and
//! then takes the first option at every new decision, recording the probabilities. Paths are
//! visited depth-first, so the total probability of all returned outcomes is exactly 1.

use crate::mechanics::Chooser;

pub struct TooLarge;

struct Replay<'p> {
    prefix: &'p [usize],
    trail: Vec<(usize, Vec<f64>)>,
}

impl Replay<'_> {
    fn pick(&mut self, probs: Vec<f64>) -> usize {
        let pos = self.trail.len();
        let i = if pos < self.prefix.len() { self.prefix[pos] } else { 0 };
        self.trail.push((i, probs));
        i
    }
}

impl Chooser for Replay<'_> {
    fn weighted(&mut self, w: &[u32]) -> usize {
        let tot: f64 = w.iter().map(|&x| x as f64).sum();
        self.pick(w.iter().map(|&x| x as f64 / tot).collect())
    }
    fn uniform(&mut self, n: usize) -> usize {
        self.pick(vec![1.0 / n as f64; n])
    }
}

/// Call `f` once per outcome path; `visit(result, probability)` receives each.
/// Returns Err(TooLarge) when more than `budget` paths would be needed.
pub fn enumerate<R>(mut f: impl FnMut(&mut dyn Chooser) -> R, budget: &mut u64, mut visit: impl FnMut(R, f64)) -> Result<(), TooLarge> {
    let mut prefix: Vec<usize> = Vec::new();
    loop {
        let mut ch = Replay { prefix: &prefix, trail: Vec::new() };
        let r = f(&mut ch);
        let p: f64 = ch.trail.iter().map(|(i, probs)| probs[*i]).product();
        visit(r, p);
        if *budget == 0 {
            return Err(TooLarge);
        }
        *budget -= 1;
        // advance to the next path: drop exhausted trailing decisions, bump the last one
        let mut trail = ch.trail;
        while let Some((i, probs)) = trail.last() {
            if *i + 1 < probs.len() {
                break;
            }
            trail.pop();
        }
        match trail.last_mut() {
            None => return Ok(()),
            Some(last) => last.0 += 1,
        }
        prefix = trail.into_iter().map(|(i, _)| i).collect();
    }
}
