//! Paging state for the Matches archive.

use crate::session::MatchSummary;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRequest {
    generation: u64,
    pub offset: usize,
}

#[derive(Clone, Debug)]
pub enum PageIssue {
    Failed(String),
    NoProgress { can_continue: bool },
}

/// One filter/refresh generation. Only an accepted response can move its server offset.
#[derive(Default)]
pub struct ArchivePages {
    pub generation: u64,
    pub rows: Vec<MatchSummary>,
    pub total: Option<u64>,
    pub issue: Option<PageIssue>,
    seen: HashSet<i64>,
    pending: Option<PageRequest>,
    next_offset: usize,
}

impl ArchivePages {
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            ..Self::default()
        }
    }

    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn has_more(&self) -> bool {
        !matches!(
            self.issue,
            Some(PageIssue::NoProgress {
                can_continue: false
            })
        ) && self
            .total
            .is_some_and(|total| (self.next_offset as u64) < total)
    }

    pub fn start(&mut self) -> Option<PageRequest> {
        if self.busy() || (self.total.is_some() && !self.has_more()) {
            return None;
        }
        let request = PageRequest {
            generation: self.generation,
            offset: self.next_offset,
        };
        self.pending = Some(request);
        self.issue = None;
        Some(request)
    }

    pub fn finish(
        &mut self,
        current_generation: u64,
        request: PageRequest,
        result: Result<(Vec<MatchSummary>, u64), String>,
    ) {
        if request.generation != current_generation || self.pending != Some(request) {
            return;
        }
        self.pending = None;
        match result {
            Ok((page, total)) => {
                let received = page.len();
                let shown = self.rows.len();
                self.next_offset += received;
                self.total = Some(total);
                self.rows
                    .extend(page.into_iter().filter(|m| self.seen.insert(m.id)));
                if self.rows.len() == shown && (received > 0 || (self.next_offset as u64) < total) {
                    self.issue = Some(PageIssue::NoProgress {
                        can_continue: received > 0 && (self.next_offset as u64) < total,
                    });
                }
            }
            Err(error) => self.issue = Some(PageIssue::Failed(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(ids: &[i64], total: u64) -> Result<(Vec<MatchSummary>, u64), String> {
        Ok((
            ids.iter()
                .map(|&id| MatchSummary {
                    id,
                    ..Default::default()
                })
                .collect(),
            total,
        ))
    }

    #[test]
    fn import_and_filter_changes_reject_stale_successes_and_errors() {
        let mut archive = ArchivePages::new(1);
        let first = archive.start().unwrap();
        archive.finish(1, first, page(&[30, 29], 5));
        let before_import = archive.start().unwrap();
        assert_eq!(before_import.offset, 2);

        // Import invalidates the current generation before the first-page effect runs.
        archive.finish(2, before_import, page(&[29, 28], 6));
        assert_eq!(archive.rows.len(), 2);
        assert_eq!(archive.total, Some(5));
        archive = ArchivePages::new(2);
        let after_import = archive.start().unwrap();
        assert_eq!(after_import.offset, 0);

        // A filter change wins over the import refresh; the late error cannot clear its in-flight page.
        archive = ArchivePages::new(3);
        let filtered = archive.start().unwrap();
        archive.finish(3, after_import, Err("late error".into()));
        assert!(archive.busy());
        archive.finish(3, filtered, page(&[99], 1));
        archive.finish(3, before_import, page(&[28, 27], 6));
        assert_eq!(archive.rows.iter().map(|m| m.id).collect::<Vec<_>>(), [99]);
        assert_eq!(archive.total, Some(1));
        assert!(archive.issue.is_none());
        assert!(archive.start().is_none());
    }

    #[test]
    fn duplicate_rows_consume_server_offset_without_being_rendered_twice() {
        let mut archive = ArchivePages::new(1);
        let first = archive.start().unwrap();
        archive.finish(1, first, page(&[5, 4], 8));
        let second = archive.start().unwrap();
        archive.finish(1, second, page(&[4, 3], 7));
        assert_eq!(
            archive.rows.iter().map(|m| m.id).collect::<Vec<_>>(),
            [5, 4, 3]
        );
        let duplicate_only = archive.start().unwrap();
        assert_eq!(duplicate_only.offset, 4);
        archive.finish(1, duplicate_only, page(&[4, 5], 7));
        assert!(matches!(
            archive.issue,
            Some(PageIssue::NoProgress { can_continue: true })
        ));
        let last = archive.start().unwrap();
        assert_eq!(last.offset, 6);
        archive.finish(1, last, page(&[1], 7));
        assert!(!archive.has_more());
        assert!(archive.issue.is_none());
    }

    #[test]
    fn failed_page_retries_same_offset_and_empty_page_can_restart() {
        let mut archive = ArchivePages::new(1);
        let first = archive.start().unwrap();
        archive.finish(1, first, page(&[5, 4], 5));
        let second = archive.start().unwrap();
        assert!(archive.start().is_none());
        archive.finish(1, second, Err("offline".into()));
        assert!(matches!(archive.issue, Some(PageIssue::Failed(ref e)) if e == "offline"));
        assert_eq!(archive.rows.len(), 2);
        let retry = archive.start().unwrap();
        assert_eq!(retry.offset, second.offset);
        archive.finish(1, retry, page(&[], 5));
        assert!(matches!(
            archive.issue,
            Some(PageIssue::NoProgress {
                can_continue: false
            })
        ));
        assert!(archive.start().is_none());
        archive = ArchivePages::new(2);
        assert_eq!(archive.start().unwrap().offset, 0);
    }

    #[test]
    fn empty_page_at_end_uses_latest_count_without_retry() {
        let mut archive = ArchivePages::new(1);
        let first = archive.start().unwrap();
        archive.finish(1, first, page(&[5, 4], 5));
        let second = archive.start().unwrap();
        archive.finish(1, second, page(&[], 2));
        assert_eq!(archive.total, Some(2));
        assert!(archive.issue.is_none());
        assert!(archive.start().is_none());
    }
}
