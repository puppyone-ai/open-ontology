//! Review queue and overlays (10 §4.8, §5.3, D22, D24). Board WS-BE3.
//!
//! * [`queue`]: which draft facts publish by themselves, which wait for a
//!   person and which stay drafts (thresholds and confidence adjustments);
//!   queue items are deduplicated and swept when they no longer apply;
//! * [`overlay`]: human decisions as append-only overlay rows; rejected facts
//!   stay out when the same content is extracted again, overlays whose
//!   evidence vanished become `orphaned_overlay` items;
//! * [`speaker`]: `speaker_identity` items for audio and video, and the merge of a speaker placeholder into the
//!   confirmed Person (D74);
//! * [`decide`]: batch decisions with a revision check per item, and the
//!   one-operation "accept everything from this source".

pub mod decide;
pub mod overlay;
pub mod queue;
pub mod speaker;
