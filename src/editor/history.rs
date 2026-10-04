use super::buffer::{EditDelta, EditorBuffer};
use super::position::{
    EditorPosition, EditorRange, EditorSelection, SelectionSet, position_after_text,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditTransaction {
    pub delta: EditDelta,
    pub before_selection: EditorSelection,
    pub after_selection: EditorSelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorHistory {
    undo_stack: Vec<RecordedTransaction>,
    redo_stack: Vec<RecordedTransaction>,
    current_revision: u64,
    clean_revision: Option<u64>,
    next_revision: u64,
    grouping_boundary: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedTransaction {
    transaction: EditTransaction,
    before_selection_set: SelectionSet,
    after_selection_set: SelectionSet,
    before_revision: u64,
    after_revision: u64,
}

impl EditorHistory {
    pub fn new(_clean_text: impl Into<String>) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_revision: 0,
            clean_revision: Some(0),
            next_revision: 1,
            grouping_boundary: 0,
        }
    }

    pub fn record(&mut self, transaction: EditTransaction) {
        if transaction.is_noop() {
            return;
        }

        self.push_recorded(
            transaction.before_selection.into(),
            transaction.after_selection.into(),
            transaction,
        );
        self.redo_stack.clear();
    }

    pub fn record_with_selection_sets(
        &mut self,
        transaction: EditTransaction,
        before_selection_set: SelectionSet,
        after_selection_set: SelectionSet,
    ) {
        if transaction.is_noop_with_sets(&before_selection_set, &after_selection_set) {
            return;
        }

        self.push_recorded(before_selection_set, after_selection_set, transaction);
        self.redo_stack.clear();
    }

    pub fn record_with_grouping(&mut self, transaction: EditTransaction) {
        if transaction.is_noop() {
            return;
        }

        if let Some(previous) = self.undo_stack.last_mut()
            && previous.after_revision != self.grouping_boundary
            && self.redo_stack.is_empty()
            && can_merge_adjacent_insert(&previous.transaction, &transaction)
        {
            let after_revision = self.next_revision;
            self.next_revision += 1;
            previous.transaction.merge_adjacent_insert(transaction);
            previous.after_selection_set = previous.transaction.after_selection.into();
            previous.after_revision = after_revision;
            self.current_revision = previous.after_revision;
            return;
        }

        self.record(transaction);
    }

    pub fn undo(&mut self, buffer: &mut EditorBuffer) -> Option<EditorSelection> {
        self.undo_selection_set(buffer).map(Into::into)
    }

    pub fn redo(&mut self, buffer: &mut EditorBuffer) -> Option<EditorSelection> {
        self.redo_selection_set(buffer).map(Into::into)
    }

    pub fn undo_selection_set(&mut self, buffer: &mut EditorBuffer) -> Option<SelectionSet> {
        let recorded = self.undo_stack.pop()?;
        let selection = recorded.before_selection_set.clone();

        buffer.replace_range(
            recorded.transaction.delta.after_range,
            &recorded.transaction.delta.before_text,
        );
        self.current_revision = recorded.before_revision;
        self.redo_stack.push(recorded);

        Some(selection)
    }

    pub fn redo_selection_set(&mut self, buffer: &mut EditorBuffer) -> Option<SelectionSet> {
        let recorded = self.redo_stack.pop()?;
        let selection = recorded.after_selection_set.clone();

        buffer.replace_range(
            recorded.transaction.delta.before_range,
            &recorded.transaction.delta.after_text,
        );
        self.current_revision = recorded.after_revision;
        self.undo_stack.push(recorded);

        Some(selection)
    }

    pub fn mark_clean(&mut self, _text: &str) {
        self.clean_revision = Some(self.current_revision);
        self.break_group();
    }

    /// Keep a save snapshot reachable by undo while its write is in flight.
    pub fn break_group(&mut self) {
        self.grouping_boundary = self.current_revision;
    }

    /// The previous disk checkpoint is no longer valid after a different snapshot is saved.
    pub fn invalidate_clean_checkpoint(&mut self) {
        self.clean_revision = None;
        self.break_group();
    }

    pub fn is_dirty(&self, _text: &str) -> bool {
        self.clean_revision != Some(self.current_revision)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    fn push_recorded(
        &mut self,
        before_selection_set: SelectionSet,
        after_selection_set: SelectionSet,
        transaction: EditTransaction,
    ) {
        let before_revision = self.current_revision;
        let after_revision = self.next_revision();

        self.current_revision = after_revision;
        self.undo_stack.push(RecordedTransaction {
            transaction,
            before_selection_set,
            after_selection_set,
            before_revision,
            after_revision,
        });
    }

    fn next_revision(&mut self) -> u64 {
        let revision = self.next_revision;
        self.next_revision += 1;
        revision
    }
}

impl EditTransaction {
    fn is_noop(&self) -> bool {
        self.delta.before_range == self.delta.after_range
            && self.delta.before_text == self.delta.after_text
            && self.before_selection == self.after_selection
    }

    fn is_noop_with_sets(
        &self,
        before_selection_set: &SelectionSet,
        after_selection_set: &SelectionSet,
    ) -> bool {
        self.delta.before_range == self.delta.after_range
            && self.delta.before_text == self.delta.after_text
            && before_selection_set == after_selection_set
    }

    fn merge_adjacent_insert(&mut self, next: EditTransaction) {
        self.delta.after_range =
            EditorRange::new(self.delta.after_range.start, next.delta.after_range.end);
        self.delta.after_text.push_str(&next.delta.after_text);
        self.after_selection = next.after_selection;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SingleInsert {
    start: EditorPosition,
    end: EditorPosition,
}

fn can_merge_adjacent_insert(previous: &EditTransaction, next: &EditTransaction) -> bool {
    if previous.after_selection != next.before_selection {
        return false;
    }

    if !previous.before_selection.is_caret()
        || !previous.after_selection.is_caret()
        || !next.after_selection.is_caret()
    {
        return false;
    }

    let Some(previous_insert) = single_insert(&previous.delta) else {
        return false;
    };
    let Some(next_insert) = single_insert(&next.delta) else {
        return false;
    };

    if previous.delta.after_text.contains(['\r', '\n'])
        || next.delta.after_text.contains(['\r', '\n'])
    {
        return false;
    }

    if previous.before_selection.cursor != previous_insert.start
        || next.before_selection.cursor != next_insert.start
    {
        return false;
    }

    next_insert.start == previous_insert.end
        && position_after_text(previous.before_selection.cursor, &previous.delta.after_text)
            == previous.after_selection.cursor
        && position_after_text(next.before_selection.cursor, &next.delta.after_text)
            == next.after_selection.cursor
}

fn single_insert(delta: &EditDelta) -> Option<SingleInsert> {
    if !delta.before_range.is_empty()
        || !delta.before_text.is_empty()
        || delta.after_range.start > delta.after_range.end
        || delta.after_text.chars().count() != 1
    {
        return None;
    }

    Some(SingleInsert {
        start: delta.before_range.start,
        end: delta.after_range.end,
    })
}
