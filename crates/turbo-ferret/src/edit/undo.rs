//! Undo and redo. TB: tb_style_edit.cpp (TBUndoEvent, TBUndoRedoStack)

use alloc::string::String;
use alloc::vec::Vec;

/// One insertion or removal. TB: TBUndoEvent
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoEvent {
    /// Offset in the whole text.
    pub gofs: usize,
    pub text: String,
    pub insert: bool,
}

/// TB: TBUndoRedoStack
#[derive(Default)]
pub struct UndoStack {
    undos: Vec<UndoEvent>,
    redos: Vec<UndoEvent>,
    /// An event is being applied: changes are not recorded.
    pub(crate) applying: bool,
}

impl UndoStack {
    pub fn can_undo(&self) -> bool {
        !self.undos.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redos.is_empty()
    }
    pub fn undos(&self) -> &[UndoEvent] {
        &self.undos
    }

    pub(crate) fn pop_undo(&mut self) -> Option<UndoEvent> {
        self.undos.pop()
    }
    pub(crate) fn pop_redo(&mut self) -> Option<UndoEvent> {
        self.redos.pop()
    }
    pub(crate) fn push_undo(&mut self, e: UndoEvent) {
        self.undos.push(e);
    }
    pub(crate) fn push_redo(&mut self, e: UndoEvent) {
        self.redos.push(e);
    }

    /// TB: Clear
    pub fn clear(&mut self, undo: bool, redo: bool) {
        if undo {
            self.undos.clear();
        }
        if redo {
            self.redos.clear();
        }
    }

    /// Record a change. Single characters typed in a row are joined into one event
    /// up to a word boundary. TB: Commit
    pub(crate) fn commit(&mut self, read_only: bool, gofs: usize, text: &str, insert: bool) {
        if self.applying || read_only {
            return;
        }
        self.clear(false, true);
        if insert && text.chars().count() == 1 {
            if let Some(e) = self.undos.last_mut() {
                if e.insert && e.gofs + e.text.len() == gofs {
                    // A space joins other text on the line; other characters only
                    // join a word.
                    let space_joins = text == " " && !e.text.contains(['\r', '\n']);
                    if space_joins || !e.text.contains([' ', '\r', '\n']) {
                        e.text.push_str(text);
                        return;
                    }
                }
            }
        }
        self.undos.push(UndoEvent { gofs, text: String::from(text), insert });
    }
}
