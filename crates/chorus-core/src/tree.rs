//! Process-tree decisions for a session's Job Object (spike S1 findings 1 and 2). The snapshot of
//! processes and windows is taken in `src-tauri::sessions::job`; these functions decide from it.

use std::collections::{BTreeSet, HashMap, HashSet};

/// One process as a snapshot sees it. `created` is the creation time in any monotonic unit
/// (Windows FILETIME ticks in practice); it guards against reused process ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub created: u64,
}

/// Which processes in a session's job to leave running when the session is closed.
///
/// An agent that opens a URL while the browser isn't running starts the browser as its own child,
/// inside the job (LESSONS L22). Close spares every process with a visible top-level window, other
/// than the session's root, together with everything under it in the job (a browser's helper
/// processes have no windows of their own). Everything else is killed.
pub fn spared_on_close(root: u32, job: &[u32], procs: &[Proc], with_windows: &HashSet<u32>) -> BTreeSet<u32> {
    let in_job: HashSet<u32> = job.iter().copied().collect();
    let children = children_by_parent(procs);
    let mut spared = BTreeSet::new();
    let mut queue: Vec<u32> = job.iter().copied().filter(|p| *p != root && with_windows.contains(p)).collect();
    while let Some(pid) = queue.pop() {
        if !spared.insert(pid) {
            continue;
        }
        for child in children.get(&pid).into_iter().flatten() {
            if in_job.contains(child) && *child != root {
                queue.push(*child);
            }
        }
    }
    spared
}

/// Descendants of `root` that are not in the job. The child is assigned to its job just after it
/// starts (portable-pty takes no job at spawn), so anything it started in that moment escaped.
/// A process only counts as a child if it was created after its parent, so a stale parent id that
/// happens to equal a reused pid is ignored.
pub fn strays(root: u32, procs: &[Proc], in_job: &HashSet<u32>) -> Vec<u32> {
    let by_pid: HashMap<u32, Proc> = procs.iter().map(|p| (p.pid, *p)).collect();
    let children = children_by_parent(procs);
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = vec![root];
    while let Some(pid) = queue.pop() {
        if !seen.insert(pid) {
            continue;
        }
        let Some(parent) = by_pid.get(&pid) else { continue };
        for child in children.get(&pid).into_iter().flatten() {
            let Some(c) = by_pid.get(child) else { continue };
            if c.created < parent.created {
                continue;
            }
            if !in_job.contains(child) {
                found.push(*child);
            }
            queue.push(*child);
        }
    }
    found.sort_unstable();
    found
}

fn children_by_parent(procs: &[Proc]) -> HashMap<u32, Vec<u32>> {
    let mut map: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in procs {
        if p.pid != p.ppid {
            map.entry(p.ppid).or_default().push(p.pid);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, created: u64) -> Proc {
        Proc { pid, ppid, created }
    }

    #[test]
    fn close_spares_a_browser_and_its_helpers_but_not_the_agent() {
        // 10 agent → 11 node, 12 browser (window) → 13, 14 helpers; 15 is a grandchild of the agent.
        let procs = [p(10, 1, 100), p(11, 10, 101), p(12, 10, 102), p(13, 12, 103), p(14, 12, 104), p(15, 11, 105)];
        let job = [10, 11, 12, 13, 14, 15];
        let windows: HashSet<u32> = [12, 10].into_iter().collect();
        let spared = spared_on_close(10, &job, &procs, &windows);
        assert_eq!(spared.into_iter().collect::<Vec<_>>(), vec![12, 13, 14]);
    }

    #[test]
    fn nothing_is_spared_without_windows_and_outsiders_are_ignored() {
        let procs = [p(10, 1, 100), p(11, 10, 101), p(50, 1, 90)];
        let windows: HashSet<u32> = [50].into_iter().collect();
        assert!(spared_on_close(10, &[10, 11], &procs, &windows).is_empty());
    }

    #[test]
    fn strays_are_descendants_outside_the_job() {
        // 10 is the root (in job). 11 escaped; 12 is 11's child (also out); 13 is in the job.
        // 20 claims parent 10 but is older than 10: a reused id, not a child.
        let procs = [p(10, 1, 100), p(11, 10, 101), p(12, 11, 102), p(13, 10, 103), p(20, 10, 50)];
        let in_job: HashSet<u32> = [10, 13].into_iter().collect();
        assert_eq!(strays(10, &procs, &in_job), vec![11, 12]);
        let all_in: HashSet<u32> = [10, 11, 12, 13].into_iter().collect();
        assert!(strays(10, &procs, &all_in).is_empty());
    }
}
