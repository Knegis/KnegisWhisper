//! Notes, folders, and transcript export.
//!
//! [`transcript`] is the interesting half. Exporting a transcript looks like
//! string formatting and is mostly judgement: merging consecutive utterances so
//! a paragraph is not forty lines, resolving speaker names in the right
//! precedence order, normalising epoch timestamps into subtitle time, and
//! knowing when a speaker label is noise rather than information.
//!
//! [`notes`] is CRUD plus full-text search, and its one subtlety is that user
//! search text is escaped into FTS5 query syntax rather than passed through.
//! Users type prose; FTS5 reads expressions.
//!
//! [`people`] is who a note was with, told apart by email (KNEG-362).
//!
//! [`trash`] is where a deleted note waits until it is emptied (KNEG-401).

pub mod document;
pub mod notes;
pub mod people;
pub mod recording_state;
pub mod recordings;
pub mod sharing;
pub mod speakers;
pub mod transcript;
pub mod trash;

pub use notes::{
    escape_query, search, Folder, NewNote, Note, NoteUpdate, SearchHit, SearchHitRow, TranscribedBy,
};
pub use recordings::NoteRecording;
pub use transcript::{export, Format, NoteMeta, Segment, SpeakerLabels, TranscriptRow, Word};

pub mod summary;

pub mod known_voices;
