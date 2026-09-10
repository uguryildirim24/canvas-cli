//! The broker: one identity, at most one attachment, and who may read it.
//!
//! Every rule REPORT §3.3 states about the lifetime of an attachment lives
//! here, and nothing here does I/O, so all of it is tested directly:
//!
//! - one active attachment per identity, bound to the browser-profile
//!   instance, the tab, the origin, the account, the identity generation, and
//!   the navigation generation;
//! - a message that names an obsolete document or navigation generation is
//!   rejected;
//! - a new document enters `validating` and the old text is erased first;
//! - cross-origin navigation ends the grant;
//! - metadata is served from the last validated state, and text only from an
//!   extract that arrived with a fresh, matching account probe;
//! - the buffers are erased on pause and on detach.

use std::collections::BTreeSet;

use uuid::Uuid;

use crate::bridge::ipc::{
    AttachmentState, AttachmentSummary, Context, FollowStatus, LoadOutcome, Reason, VerifiedAccount,
};
use crate::bridge::note::{self, Note};
use crate::bridge::text::{bound_extract, extract_bytes, sanitize_url};
use crate::bridge::wire::{Account, Extract, Observation, PauseCause, Route, Zone, classify_route};

/// Browser context is never cacheable (REPORT §3.2): its TTL is always zero.
pub const BROWSER_TTL_MS: u64 = 0;

/// The identity one broker owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedIdentity {
    pub key: String,
    pub origin: String,
    pub user_id: i64,
    pub generation: String,
}

/// What the broker did with one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Accepted {
    /// A new attachment exists.
    Attached { attachment_id: String },
    /// The state changed and nothing needs to be said back.
    Updated,
    /// The message was refused; nothing changed and no text was accepted.
    Refused { reason: Reason },
    /// The attachment is gone.
    Ended,
}

/// One live attachment.
#[derive(Debug, Clone)]
struct Attachment {
    id: String,
    profile_instance: String,
    tab_id: i64,
    origin: String,
    document_id: String,
    frame_id: i64,
    navigation_generation: u64,
    identity_generation: String,
    account: VerifiedAccount,
    zone: Zone,
    route: Route,
    url: Option<String>,
    title: Option<String>,
    observed_at: String,
    attached_at: String,
    state: AttachmentState,
    consumers: BTreeSet<String>,
    /// The extract of **this** document, or nothing.
    extract: Option<Extract>,
    /// The document and navigation generation the extract belongs to.
    extract_at: Option<(String, u64)>,
    /// The notes held for this attachment, oldest first.
    ///
    /// They live as long as the attachment does and no longer: nothing here
    /// is written to disk, and dropping the attachment drops them with it.
    notes: Vec<Note>,
    /// The last navigation asked for, per consumer.
    ///
    /// One consumer's navigation is not another's business, so a bundle
    /// carries only the follow that consumer asked for. The CLI, which names
    /// no consumer, reads its own under the empty name.
    follows: std::collections::BTreeMap<String, FollowStatus>,
}

impl Attachment {
    /// Forget every byte of page content (pause, detach, new document).
    fn erase(&mut self) {
        self.extract = None;
        self.extract_at = None;
    }
}

/// One identity's broker.
#[derive(Debug)]
pub struct Broker {
    identity: OwnedIdentity,
    /// The extension id `bridge.extension_id` names, when one is configured.
    extension_id: Option<String>,
    /// The browser-profile instance of the connected port.
    profile_instance: Option<String>,
    attachment: Option<Attachment>,
}

impl Broker {
    /// A broker that owns one identity and has no port yet.
    #[must_use]
    pub fn new(identity: OwnedIdentity, extension_id: Option<String>) -> Self {
        Self {
            identity,
            extension_id,
            profile_instance: None,
            attachment: None,
        }
    }

    /// The identity this broker owns.
    #[must_use]
    pub fn identity(&self) -> &OwnedIdentity {
        &self.identity
    }

    /// Accept a port from the companion.
    ///
    /// The extension id is checked here as well as by Chrome's
    /// `allowed_origins`: the manifest is a file on disk that a different
    /// installation could have rewritten, and this is the check the host owns.
    pub fn hello(&mut self, extension_id: &str, profile_instance: &str) -> Result<(), Reason> {
        if let Some(expected) = &self.extension_id
            && expected != extension_id
        {
            return Err(Reason::Protocol);
        }
        if !is_extension_id(extension_id) || profile_instance.is_empty() {
            return Err(Reason::Protocol);
        }
        self.profile_instance = Some(profile_instance.to_owned());
        Ok(())
    }

    /// Whether a port has been accepted.
    #[must_use]
    pub fn has_port(&self) -> bool {
        self.profile_instance.is_some()
    }

    /// The person invoked the companion: create the one attachment.
    pub fn attach(&mut self, observation: &Observation) -> Accepted {
        let Some(profile_instance) = self.profile_instance.clone() else {
            return Accepted::Refused {
                reason: Reason::Protocol,
            };
        };
        if observation.origin != self.identity.origin {
            // A tab on another origin is not this identity's Canvas.
            return Accepted::Refused {
                reason: Reason::AccountMismatch,
            };
        }
        let account = match self.verify(observation.account.as_ref()) {
            Ok(account) => account,
            Err(reason) => {
                return Accepted::Refused { reason };
            }
        };
        let zone = Self::zone_of(observation);
        let id = new_attachment_id();
        let (route, url, title) = Self::visible(observation, zone);
        self.attachment = Some(Attachment {
            id: id.clone(),
            profile_instance,
            tab_id: observation.tab_id,
            origin: observation.origin.clone(),
            document_id: observation.document_id.clone(),
            frame_id: observation.frame_id,
            navigation_generation: observation.navigation_generation,
            identity_generation: self.identity.generation.clone(),
            account,
            zone,
            route,
            url,
            title,
            observed_at: observation.observed_at.clone(),
            attached_at: observation.observed_at.clone(),
            state: AttachmentState::Attached,
            consumers: BTreeSet::new(),
            extract: None,
            extract_at: None,
            notes: Vec::new(),
            follows: std::collections::BTreeMap::new(),
        });
        Accepted::Attached { attachment_id: id }
    }

    /// A committed navigation, a re-probe, or a resume.
    pub fn update(&mut self, observation: &Observation) -> Accepted {
        let Some(current) = self.attachment.as_ref() else {
            return Accepted::Refused {
                reason: Reason::NotAttached,
            };
        };
        // Cross-origin navigation ends the grant: `activeTab` is gone and a
        // new gesture is required (REPORT §3.3 step 6).
        if observation.origin != current.origin || observation.origin != self.identity.origin {
            self.attachment = None;
            return Accepted::Ended;
        }
        if observation.tab_id != current.tab_id {
            return Accepted::Refused {
                reason: Reason::Protocol,
            };
        }
        // The attachment is bound to one browser-profile instance: a port
        // that rebound to another profile cannot inherit it.
        if self.profile_instance.as_deref() != Some(current.profile_instance.as_str()) {
            self.attachment = None;
            return Accepted::Ended;
        }
        // A message from behind the current document or navigation is late.
        if observation.navigation_generation < current.navigation_generation {
            return Accepted::Refused {
                reason: Reason::StaleGeneration,
            };
        }
        let new_document = observation.document_id != current.document_id
            || observation.navigation_generation > current.navigation_generation;
        let account = match self.verify(observation.account.as_ref()) {
            Ok(account) => Some(account),
            // An unverifiable account never joins API and browser content: the
            // attachment survives, paused, with every byte erased.
            Err(reason) => {
                if let Some(current) = self.attachment.as_mut() {
                    current.erase();
                    current.state = AttachmentState::Paused;
                }
                return Accepted::Refused { reason };
            }
        };
        let zone = Self::zone_of(observation);
        let (route, url, title) = Self::visible(observation, zone);
        let Some(current) = self.attachment.as_mut() else {
            return Accepted::Refused {
                reason: Reason::NotAttached,
            };
        };
        if new_document {
            // §3.3 step 6: erase first, then re-check.
            current.erase();
        }
        current.document_id.clone_from(&observation.document_id);
        current.frame_id = observation.frame_id;
        current.navigation_generation = observation.navigation_generation;
        current.observed_at.clone_from(&observation.observed_at);
        current.zone = zone;
        current.route = route;
        current.url = url;
        current.title = title;
        if let Some(account) = account {
            current.account = account;
        }
        current.state = if zone == Zone::Assessment {
            // Entering an assessment pauses sharing (REPORT §3.3 step 6).
            current.erase();
            AttachmentState::Paused
        } else if new_document {
            AttachmentState::Validating
        } else {
            AttachmentState::Attached
        };
        Accepted::Updated
    }

    /// Finish validation of the current document.
    ///
    /// A document that arrived as `validating` becomes `attached` only when a
    /// matching account probe accompanies it.
    pub fn validated(&mut self) {
        if let Some(current) = self.attachment.as_mut()
            && current.state == AttachmentState::Validating
        {
            current.state = AttachmentState::Attached;
        }
    }

    /// The answer to a text request.
    ///
    /// The extract is accepted only when the account probe that came with it
    /// matches this identity, and only when it belongs to the document and
    /// navigation generation the broker currently holds.
    pub fn text(
        &mut self,
        document_id: &str,
        navigation_generation: u64,
        account: Option<&Account>,
        zone: Zone,
        extract: Extract,
    ) -> Accepted {
        let verified = match self.verify(account) {
            Ok(account) => account,
            Err(reason) => {
                if let Some(current) = self.attachment.as_mut() {
                    current.erase();
                }
                return Accepted::Refused { reason };
            }
        };
        let Some(current) = self.attachment.as_mut() else {
            return Accepted::Refused {
                reason: Reason::NotAttached,
            };
        };
        if current.document_id != document_id
            || current.navigation_generation != navigation_generation
        {
            // Late text from a page that is gone is never stored.
            return Accepted::Refused {
                reason: Reason::StaleGeneration,
            };
        }
        if zone.is_opaque() || current.zone.is_opaque() {
            current.erase();
            return Accepted::Refused {
                reason: Reason::ZoneOpaque,
            };
        }
        let mut extract = extract;
        bound_extract(&mut extract);
        current.account = verified;
        current.extract_at = Some((document_id.to_owned(), navigation_generation));
        current.extract = Some(extract);
        if current.state == AttachmentState::Validating {
            current.state = AttachmentState::Attached;
        }
        Accepted::Updated
    }

    /// Sharing is suspended. Every byte of page content is erased.
    pub fn pause(&mut self, cause: PauseCause) -> Accepted {
        match cause {
            PauseCause::TabClosed | PauseCause::UserDetached | PauseCause::CrossOrigin => {
                self.detach_all()
            }
            PauseCause::Hidden | PauseCause::Assessment => {
                if let Some(current) = self.attachment.as_mut() {
                    current.erase();
                    current.state = AttachmentState::Paused;
                    Accepted::Updated
                } else {
                    Accepted::Refused {
                        reason: Reason::NotAttached,
                    }
                }
            }
        }
    }

    /// End the attachment and forget every consumer of it.
    pub fn detach_all(&mut self) -> Accepted {
        if self.attachment.take().is_some() {
            Accepted::Ended
        } else {
            Accepted::Refused {
                reason: Reason::NotAttached,
            }
        }
    }

    /// End one attachment by its id, or the sole one when none is named.
    pub fn detach(&mut self, attachment_id: Option<&str>) -> Result<(), Reason> {
        match (self.attachment.as_ref(), attachment_id) {
            (Some(current), Some(id)) if current.id != id => Err(Reason::NotAttached),
            (Some(_), _) => {
                self.attachment = None;
                Ok(())
            }
            (None, _) => Err(Reason::NotAttached),
        }
    }

    /// Give up one consumer's opt-in, leaving the attachment alone.
    ///
    /// A consumer that never attached is `NotAttached`, so `context.detach`
    /// cannot be used to learn whether some other consumer is attached.
    pub fn detach_consumer(
        &mut self,
        attachment_id: Option<&str>,
        consumer: &str,
    ) -> Result<(), Reason> {
        let Some(current) = self.attachment.as_mut() else {
            return Err(Reason::NotAttached);
        };
        if attachment_id.is_some_and(|id| current.id != id) {
            return Err(Reason::NotAttached);
        }
        if !current.consumers.remove(consumer) {
            return Err(Reason::NotAttached);
        }
        Ok(())
    }

    /// Opt one consumer in and hand it the capability.
    ///
    /// `attachment_id` is optional here because a consumer that has never
    /// attached holds no capability yet. When it is given it must match, so
    /// a stale handle opts nobody in.
    pub fn attach_consumer(
        &mut self,
        attachment_id: Option<&str>,
        consumer: &str,
    ) -> Result<(String, AttachmentState), Reason> {
        if consumer.is_empty() {
            return Err(Reason::Protocol);
        }
        let Some(current) = self.attachment.as_mut() else {
            return Err(Reason::NotAttached);
        };
        if attachment_id.is_some_and(|id| current.id != id) {
            return Err(Reason::NotAttached);
        }
        current.consumers.insert(consumer.to_owned());
        Ok((current.id.clone(), current.state))
    }

    /// What `attachments.list` reports: no capability and no page content.
    #[must_use]
    pub fn list(&self) -> Vec<AttachmentSummary> {
        self.attachment
            .iter()
            .map(|current| AttachmentSummary {
                state: current.state,
                origin: current.origin.clone(),
                account_user_id: current.account.user_id.clone(),
                zone: current.zone,
                consumers: current.consumers.iter().cloned().collect(),
                attached_at: current.attached_at.clone(),
                navigation_generation: current.navigation_generation,
            })
            .collect()
    }

    /// Whether an extract for the current document is already held.
    ///
    /// The host asks before it sends a text request, so a metadata read never
    /// triggers a probe (REPORT §3.3 step 4).
    #[must_use]
    pub fn has_text(&self) -> bool {
        self.attachment.as_ref().is_some_and(|current| {
            current.extract.is_some()
                && current.extract_at.as_ref().is_some_and(|(document, nav)| {
                    *document == current.document_id && *nav == current.navigation_generation
                })
        })
    }

    /// The attachment id, for the host's own text requests.
    #[must_use]
    pub fn attachment_id(&self) -> Option<&str> {
        self.attachment.as_ref().map(|current| current.id.as_str())
    }

    /// The document and navigation generation a text request must name.
    #[must_use]
    pub fn current_document(&self) -> Option<(String, u64)> {
        self.attachment
            .as_ref()
            .map(|current| (current.document_id.clone(), current.navigation_generation))
    }

    /// Whether this caller may read the attachment at all.
    ///
    /// This is the capability check alone, and it is separate from `context`
    /// so the host can make it **before** it asks the browser for anything: a
    /// caller that may not read must not be able to cause a page extraction
    /// or an account probe (REPORT §3.3 step 4).
    ///
    /// `attachment_id` is the capability. `consumer`, which the adapter sets
    /// and a model cannot, selects the attachment that consumer opted into.
    /// With neither — the CLI — the sole attachment is served, which is what
    /// REPORT §3.2 permits the CLI and nothing else.
    pub fn may_read(
        &self,
        attachment_id: Option<&str>,
        consumer: Option<&str>,
    ) -> Result<(), Reason> {
        let current = self.attachment.as_ref().ok_or(Reason::NotAttached)?;
        match (attachment_id, consumer) {
            (Some(id), _) => {
                if id != current.id {
                    return Err(Reason::NotAttached);
                }
                if let Some(consumer) = consumer
                    && !current.consumers.contains(consumer)
                {
                    return Err(Reason::NotAttached);
                }
            }
            (None, Some(consumer)) => {
                if !current.consumers.contains(consumer) {
                    return Err(Reason::NotAttached);
                }
            }
            // The CLI, selecting the sole attachment.
            (None, None) => {}
        }
        Ok(())
    }

    /// Resolve a `here` request to the browser section of the bundle.
    ///
    /// The caller is checked by [`Broker::may_read`] first, so nothing below
    /// is reached by a caller that holds neither the capability nor an
    /// opt-in.
    pub fn context(
        &self,
        attachment_id: Option<&str>,
        consumer: Option<&str>,
        include_text: bool,
    ) -> Result<Context, Reason> {
        self.may_read(attachment_id, consumer)?;
        let current = self.attachment.as_ref().ok_or(Reason::NotAttached)?;
        match current.state {
            AttachmentState::Paused => return Err(Reason::Paused),
            AttachmentState::Validating => return Err(Reason::Validating),
            AttachmentState::Attached => {}
        }
        if current.identity_generation != self.identity.generation {
            return Err(Reason::StaleGeneration);
        }

        let opaque = current.zone.is_opaque();
        let fresh = current.extract_at.as_ref().is_some_and(|(document, nav)| {
            *document == current.document_id && *nav == current.navigation_generation
        });
        let (selection, text, truncated, content_reason) = if opaque {
            (None, None, false, Some(Reason::ZoneOpaque))
        } else if !include_text {
            (None, None, false, None)
        } else if fresh && let Some(extract) = current.extract.as_ref() {
            (
                extract.selection.clone(),
                extract.text.clone(),
                extract.truncated,
                None,
            )
        } else {
            // No extract for this document: never recycle an earlier page's.
            (None, None, false, Some(Reason::Validating))
        };
        let bytes = |value: &Option<String>| value.as_ref().map_or(0, String::len) as u64;
        // An opaque zone exposes no route at all; the attachment stored one
        // only if the zone was open when the document arrived.
        let route = if opaque {
            Route::opaque()
        } else {
            current.route.clone()
        };
        Ok(Context {
            attachment_id: current.id.clone(),
            state: current.state,
            consumers: current.consumers.iter().cloned().collect(),
            origin: current.origin.clone(),
            account: current.account.clone(),
            zone: current.zone,
            page_kind: route.kind,
            course_id: route.course_id,
            assignment_id: route.assignment_id,
            topic_id: route.topic_id,
            quiz_id: route.quiz_id,
            page_url: route.page_url,
            url: current.url.clone(),
            title: current.title.clone(),
            document_id: current.document_id.clone(),
            frame_id: current.frame_id,
            navigation_generation: current.navigation_generation,
            observed_at: current.observed_at.clone(),
            ttl_ms: BROWSER_TTL_MS,
            selection_bytes: bytes(&selection),
            text_bytes: bytes(&text),
            selection,
            text,
            truncated,
            content_reason,
            follow: current.follows.get(consumer.unwrap_or_default()).cloned(),
            notes: current.notes.clone(),
        })
    }

    /// Whether a message written against `generation` still describes the
    /// document the person is on.
    ///
    /// Notes and navigation are both bound to the navigation generation the
    /// agent last read (REPORT §3.2, §3.3 step 6). Any other value — behind
    /// or ahead — is stale: the agent is talking about a page that is not the
    /// one in front of the person.
    pub fn check_generation(&self, generation: u64) -> Result<(), Reason> {
        let current = self.attachment.as_ref().ok_or(Reason::NotAttached)?;
        if current.navigation_generation == generation {
            Ok(())
        } else {
            Err(Reason::StaleGeneration)
        }
    }

    /// The origin the attachment is bound to.
    #[must_use]
    pub fn origin(&self) -> &str {
        self.attachment
            .as_ref()
            .map_or(self.identity.origin.as_str(), |c| c.origin.as_str())
    }

    /// Hold one inert note for the attachment.
    ///
    /// The caller is checked first, then the generation, then the bounds. A
    /// note never changes the attachment's state, never releases text, and
    /// never touches a plan: it is added to a list the panel displays.
    pub fn note(
        &mut self,
        attachment_id: Option<&str>,
        consumer: Option<&str>,
        generation: u64,
        text: &str,
        source_refs: &[String],
        at: &str,
    ) -> Result<Note, Reason> {
        self.may_read(attachment_id, consumer)?;
        self.check_generation(generation)?;
        let origin = self.origin().to_owned();
        let held = self.attachment.as_ref().map_or(0, |c| c.notes.len());
        note::check(text, source_refs, &origin, held)?;
        let current = self.attachment.as_mut().ok_or(Reason::NotAttached)?;
        let note = Note {
            note_id: new_attachment_id(),
            // The CLI names no consumer; the panel still says who wrote it.
            consumer: consumer.unwrap_or("cli").to_owned(),
            text: text.to_owned(),
            source_refs: source_refs.to_vec(),
            at: at.to_owned(),
            generation,
        };
        current.notes.push(note.clone());
        Ok(note)
    }

    /// The notes held for the attachment, oldest first.
    #[must_use]
    pub fn notes(&self) -> Vec<Note> {
        self.attachment
            .as_ref()
            .map(|current| current.notes.clone())
            .unwrap_or_default()
    }

    /// Check a `follow` before the browser is asked to go anywhere.
    ///
    /// The caller's capability, the generation, and the granted origin are
    /// all settled here, so an unentitled caller and a cross-origin target
    /// both cost the browser nothing.
    pub fn may_follow(
        &self,
        attachment_id: Option<&str>,
        consumer: Option<&str>,
        generation: u64,
        url: &str,
    ) -> Result<(), Reason> {
        self.may_read(attachment_id, consumer)?;
        self.check_generation(generation)?;
        let granted = reqwest::Url::parse(self.origin()).map_err(|_| Reason::OriginMismatch)?;
        let target = reqwest::Url::parse(url).map_err(|_| Reason::OriginMismatch)?;
        if target.scheme() != "https" || target.origin() != granted.origin() {
            return Err(Reason::OriginMismatch);
        }
        // The resolver already refuses these; the host refuses them again,
        // because the host is the side that asks the browser to move.
        if !target.username().is_empty() || target.password().is_some() {
            return Err(Reason::OriginMismatch);
        }
        Ok(())
    }

    /// Record that the companion accepted a navigation.
    pub fn dispatched(&mut self, consumer: Option<&str>, follow: FollowStatus) {
        if let Some(current) = self.attachment.as_mut() {
            current
                .follows
                .insert(consumer.unwrap_or_default().to_owned(), follow);
        }
    }

    /// Record what became of an accepted navigation.
    ///
    /// The outcome is matched by request id, so a late answer for a
    /// navigation that was already replaced updates nothing.
    pub fn navigated(&mut self, request_id: &str, outcome: LoadOutcome, at: &str) -> bool {
        let Some(current) = self.attachment.as_mut() else {
            return false;
        };
        for follow in current.follows.values_mut() {
            if follow.request_id == request_id {
                follow.load = outcome;
                follow.load_at = Some(at.to_owned());
                return true;
            }
        }
        false
    }

    /// The last navigation this consumer asked for.
    #[must_use]
    pub fn follow_of(&self, consumer: Option<&str>) -> Option<FollowStatus> {
        self.attachment
            .as_ref()
            .and_then(|current| current.follows.get(consumer.unwrap_or_default()).cloned())
    }

    /// Check the probed account against this identity (REPORT §3.3 step 2).
    fn verify(&self, account: Option<&Account>) -> Result<VerifiedAccount, Reason> {
        // A failed or redirected probe sends no account at all: without one,
        // nothing is joined and no text is accepted.
        let account = account.ok_or(Reason::AccountMismatch)?;
        if account.user_id != self.identity.user_id.to_string() {
            return Err(Reason::AccountMismatch);
        }
        Ok(VerifiedAccount {
            user_id: account.user_id.clone(),
            observed_at: account.observed_at.clone(),
        })
    }

    /// The zone the host is willing to believe.
    ///
    /// The extension classifies from the document, which the host cannot see.
    /// The host classifies from the sanitized path, which the extension could
    /// have lied about. The stricter of the two wins.
    ///
    /// An opaque zone carries no URL at all, by design, so there is no path to
    /// classify from. The host's own reading is then `unknown`, which is
    /// opaque and cannot loosen anything — but taking the stricter of it and
    /// `assessment` would report the page as merely unrecognized and lose the
    /// one distinction that pauses sharing. With no URL, an already-opaque
    /// classification stands as it is and anything else reads `unknown`.
    fn zone_of(observation: &Observation) -> Zone {
        let Some(url) = observation
            .url
            .as_deref()
            .and_then(sanitize_url)
            .and_then(|url| reqwest::Url::parse(&url).ok())
        else {
            return if observation.zone.is_opaque() {
                observation.zone
            } else {
                Zone::Unknown
            };
        };
        let from_route = crate::bridge::wire::zone_for_route(&classify_route(url.path()));
        observation.zone.strictest(from_route)
    }

    /// What may be kept about a document, given its zone.
    ///
    /// An opaque zone exposes nothing: no route ids, no URL, no title.
    fn visible(observation: &Observation, zone: Zone) -> (Route, Option<String>, Option<String>) {
        if zone.is_opaque() {
            return (Route::opaque(), None, None);
        }
        let url = observation.url.as_deref().and_then(sanitize_url);
        // The route the host derives from the sanitized path, not the one the
        // extension asserted: an id that reaches an API handler is the host's.
        let route = url
            .as_deref()
            .and_then(|url| reqwest::Url::parse(url).ok())
            .map(|url| classify_route(url.path()))
            .unwrap_or_default();
        let title = observation
            .title
            .as_deref()
            .map(|title| crate::bridge::text::truncate_utf8(title, 512).0.to_owned());
        (route, url, title)
    }
}

/// Whether a string is shaped like a Chrome extension id.
///
/// Chrome ids are 32 characters from `a`–`p`.
#[must_use]
pub fn is_extension_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// A fresh opaque 128-bit attachment id, as 32 hex characters.
#[must_use]
pub fn new_attachment_id() -> String {
    Uuid::new_v4().simple().to_string()
}

/// The number of bytes an extract holds, for tests and reporting.
#[must_use]
pub fn payload_bytes(extract: &Extract) -> usize {
    extract_bytes(extract)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::wire::{PageKind, classify_route};

    const ORIGIN: &str = "https://school.test";
    const EXTENSION: &str = "abcdefghijklmnopabcdefghijklmnop";

    fn identity() -> OwnedIdentity {
        OwnedIdentity {
            key: "school.test-12345-deadbeef".to_owned(),
            origin: ORIGIN.to_owned(),
            user_id: 12345,
            generation: "11111111-1111-4111-8111-111111111111".to_owned(),
        }
    }

    fn account(user_id: &str) -> Account {
        Account {
            user_id: user_id.to_owned(),
            observed_at: "2026-09-10T10:00:00Z".to_owned(),
        }
    }

    fn observation(path: &str, document: &str, nav: u64) -> Observation {
        let url = format!("{ORIGIN}{path}");
        let route = classify_route(path);
        Observation {
            origin: ORIGIN.to_owned(),
            tab_id: 7,
            document_id: document.to_owned(),
            frame_id: 0,
            navigation_generation: nav,
            zone: crate::bridge::wire::zone_for_route(&route),
            route,
            account: Some(account("12345")),
            url: Some(url),
            title: Some("Essay 1".to_owned()),
            observed_at: "2026-09-10T10:00:00Z".to_owned(),
        }
    }

    fn extract(text: &str) -> Extract {
        Extract {
            selection: None,
            text: Some(text.to_owned()),
            truncated: false,
        }
    }

    fn broker() -> Broker {
        let mut broker = Broker::new(identity(), Some(EXTENSION.to_owned()));
        broker.hello(EXTENSION, "profile-a").expect("a port");
        broker
    }

    fn attached() -> (Broker, String) {
        let mut broker = broker();
        let Accepted::Attached { attachment_id } =
            broker.attach(&observation("/courses/1/assignments/2", "doc-1", 1))
        else {
            panic!("the attachment must exist");
        };
        (broker, attachment_id)
    }

    // ---------------------------------------------------------- identity

    /// M7-a acceptance: an exact extension id, and nothing else.
    #[test]
    fn a_wrong_extension_id_is_refused() {
        let mut broker = Broker::new(identity(), Some(EXTENSION.to_owned()));
        assert_eq!(
            broker.hello("ponmlkjihgfedcbaponmlkjihgfedcba", "profile-a"),
            Err(Reason::Protocol)
        );
        assert!(!broker.has_port());
        // A shape that is not a Chrome id at all.
        let mut open = Broker::new(identity(), None);
        assert_eq!(open.hello("not-an-id", "profile-a"), Err(Reason::Protocol));
        assert_eq!(open.hello(EXTENSION, ""), Err(Reason::Protocol));
        assert!(open.hello(EXTENSION, "profile-a").is_ok());
    }

    /// M7-a acceptance: a tab on another origin never attaches.
    #[test]
    fn a_foreign_origin_never_attaches() {
        let mut broker = broker();
        let mut foreign = observation("/courses/1", "doc-1", 1);
        foreign.origin = "https://evil.test".to_owned();
        foreign.url = Some("https://evil.test/courses/1".to_owned());
        assert_eq!(
            broker.attach(&foreign),
            Accepted::Refused {
                reason: Reason::AccountMismatch
            }
        );
        assert!(broker.list().is_empty());
    }

    /// M7-a acceptance: account mismatch refuses the attachment.
    #[test]
    fn a_different_account_refuses_the_attachment() {
        let mut broker = broker();
        let mut other = observation("/courses/1", "doc-1", 1);
        other.account = Some(account("999"));
        assert_eq!(
            broker.attach(&other),
            Accepted::Refused {
                reason: Reason::AccountMismatch
            }
        );
        assert!(broker.list().is_empty());
    }

    /// M7-a acceptance: a failed or redirected probe refuses everything.
    ///
    /// The extension sends no account when the probe failed or was
    /// redirected, and no account means nothing is joined.
    #[test]
    fn a_failed_probe_refuses_the_attachment_and_the_text() {
        let mut broker = broker();
        let mut unprobed = observation("/courses/1", "doc-1", 1);
        unprobed.account = None;
        assert_eq!(
            broker.attach(&unprobed),
            Accepted::Refused {
                reason: Reason::AccountMismatch
            }
        );

        let (mut broker, _) = attached();
        assert_eq!(
            broker.text("doc-1", 1, None, Zone::Open, extract("the essay prompt")),
            Accepted::Refused {
                reason: Reason::AccountMismatch
            }
        );
        assert!(!broker.has_text());
        let context = broker.context(None, None, true).expect("metadata survives");
        assert_eq!(context.text, None);
    }

    // --------------------------------------------------------- lifetime

    #[test]
    fn one_identity_holds_one_attachment() {
        let (mut broker, first) = attached();
        let Accepted::Attached {
            attachment_id: second,
        } = broker.attach(&observation("/courses/9", "doc-9", 1))
        else {
            panic!("a second gesture replaces the first");
        };
        assert_ne!(first, second);
        assert_eq!(broker.list().len(), 1);
        assert_eq!(broker.detach(Some(&first)), Err(Reason::NotAttached));
    }

    #[test]
    fn an_attachment_id_is_an_opaque_128_bit_value() {
        let (_, id) = attached();
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(id, new_attachment_id());
    }

    /// M7-a acceptance: cross-origin navigation revokes the grant.
    #[test]
    fn cross_origin_navigation_ends_the_attachment() {
        let (mut broker, _) = attached();
        let mut away = observation("/courses/1", "doc-2", 2);
        away.origin = "https://elsewhere.test".to_owned();
        assert_eq!(broker.update(&away), Accepted::Ended);
        assert!(broker.list().is_empty());
        assert_eq!(broker.context(None, None, false), Err(Reason::NotAttached));
    }

    /// M7-a acceptance: a new document erases the old text and validates.
    #[test]
    fn a_new_document_erases_the_text_and_enters_validating() {
        let (mut broker, _) = attached();
        broker.text(
            "doc-1",
            1,
            Some(&account("12345")),
            Zone::Open,
            extract("page one"),
        );
        assert!(broker.has_text());

        assert_eq!(
            broker.update(&observation("/courses/1/assignments/3", "doc-2", 2)),
            Accepted::Updated
        );
        assert!(!broker.has_text(), "old text is erased first");
        assert_eq!(broker.context(None, None, true), Err(Reason::Validating));

        broker.validated();
        let context = broker.context(None, None, true).expect("validated");
        assert_eq!(context.text, None, "never recycle an earlier page's text");
        assert_eq!(context.content_reason, Some(Reason::Validating));
        assert_eq!(context.assignment_id.as_deref(), Some("3"));
    }

    /// M7-a acceptance: a message from an obsolete generation is rejected.
    #[test]
    fn stale_document_and_navigation_messages_are_rejected() {
        let (mut broker, _) = attached();
        assert_eq!(
            broker.update(&observation("/courses/1", "doc-0", 0)),
            Accepted::Refused {
                reason: Reason::StaleGeneration
            }
        );
        // Text that names the page that is gone.
        broker.update(&observation("/courses/1/assignments/3", "doc-2", 2));
        broker.validated();
        assert_eq!(
            broker.text(
                "doc-1",
                1,
                Some(&account("12345")),
                Zone::Open,
                extract("gone")
            ),
            Accepted::Refused {
                reason: Reason::StaleGeneration
            }
        );
        assert_eq!(
            broker.text(
                "doc-2",
                1,
                Some(&account("12345")),
                Zone::Open,
                extract("gone")
            ),
            Accepted::Refused {
                reason: Reason::StaleGeneration
            }
        );
        assert!(!broker.has_text());
    }

    #[test]
    fn a_hidden_pause_erases_the_buffers() {
        let (mut broker, _) = attached();
        broker.text(
            "doc-1",
            1,
            Some(&account("12345")),
            Zone::Open,
            extract("the prompt"),
        );
        assert_eq!(broker.pause(PauseCause::Hidden), Accepted::Updated);
        assert!(!broker.has_text());
        assert_eq!(broker.context(None, None, true), Err(Reason::Paused));
    }

    #[test]
    fn a_closed_tab_ends_the_attachment() {
        let (mut broker, _) = attached();
        assert_eq!(broker.pause(PauseCause::TabClosed), Accepted::Ended);
        assert_eq!(broker.context(None, None, false), Err(Reason::NotAttached));
    }

    #[test]
    fn a_second_browser_profile_cannot_inherit_the_attachment() {
        let (mut broker, _) = attached();
        broker.hello(EXTENSION, "profile-b").expect("a second port");
        assert_eq!(
            broker.update(&observation("/courses/1/assignments/2", "doc-1", 1)),
            Accepted::Ended
        );
    }

    // ------------------------------------------------------------- zones

    /// M7-a acceptance: nothing is captured before zone classification, and
    /// an opaque zone exposes nothing at all.
    #[test]
    fn an_assessment_exposes_nothing_and_pauses() {
        let (mut broker, _) = attached();
        broker.text(
            "doc-1",
            1,
            Some(&account("12345")),
            Zone::Open,
            extract("the prompt"),
        );
        assert_eq!(
            broker.update(&observation("/courses/1/quizzes/5/take", "doc-2", 2)),
            Accepted::Updated
        );
        assert!(!broker.has_text());
        assert_eq!(broker.context(None, None, true), Err(Reason::Paused));
        let summary = &broker.list()[0];
        assert_eq!(summary.zone, Zone::Assessment);
        assert_eq!(summary.state, AttachmentState::Paused);
    }

    /// An assessment the companion reported as one stays an assessment.
    ///
    /// The extension sends no URL from an opaque zone, so the host has no path
    /// of its own to classify. It must not read that silence as "merely
    /// unrecognized": entering an assessment is what pauses sharing.
    #[test]
    fn an_assessment_that_carries_no_url_is_still_an_assessment() {
        let (mut broker, _) = attached();
        broker.text(
            "doc-1",
            1,
            Some(&account("12345")),
            Zone::Open,
            extract("the prompt"),
        );
        // Exactly what `content.js` sends for an opaque document.
        let mut opaque = observation("/courses/1/quizzes/5/take", "doc-2", 2);
        opaque.zone = Zone::Assessment;
        opaque.route = Route::opaque();
        opaque.url = None;
        opaque.title = None;
        assert_eq!(broker.update(&opaque), Accepted::Updated);

        let summary = &broker.list()[0];
        assert_eq!(summary.zone, Zone::Assessment, "the assessment was lost");
        assert_eq!(summary.state, AttachmentState::Paused);
        assert!(!broker.has_text());
        assert_eq!(broker.context(None, None, true), Err(Reason::Paused));

        // A zone that is not opaque still cannot be believed without a path.
        let mut lying = observation("/courses/1/assignments/2", "doc-3", 3);
        lying.url = None;
        let mut second = Broker::new(identity(), Some(EXTENSION.to_owned()));
        second.hello(EXTENSION, "profile-b").expect("a port");
        assert!(matches!(second.attach(&lying), Accepted::Attached { .. }));
        assert_eq!(second.list()[0].zone, Zone::Unknown);
    }

    #[test]
    fn an_opaque_zone_carries_no_route_no_url_and_no_title() {
        let mut broker = broker();
        let mut unknown = observation("/lti/tool/launch", "doc-1", 1);
        unknown.zone = Zone::Unknown;
        assert!(matches!(broker.attach(&unknown), Accepted::Attached { .. }));
        let context = broker.context(None, None, true).expect("metadata only");
        assert_eq!(context.zone, Zone::Unknown);
        assert_eq!(context.url, None);
        assert_eq!(context.title, None);
        assert_eq!(context.page_kind, None);
        assert_eq!(context.course_id, None);
        assert_eq!(context.text, None);
        assert_eq!(context.selection, None);
        assert_eq!(context.content_reason, Some(Reason::ZoneOpaque));
        assert_eq!(context.origin, ORIGIN, "the origin is not page content");
    }

    /// The extension cannot talk the host into a more permissive zone.
    #[test]
    fn the_stricter_of_the_two_classifications_wins() {
        let mut broker = broker();
        let mut lying = observation("/courses/1/quizzes/5", "doc-1", 1);
        lying.zone = Zone::Open;
        assert!(matches!(broker.attach(&lying), Accepted::Attached { .. }));
        assert_eq!(broker.list()[0].zone, Zone::Assessment);

        // And the other direction: a route that reads open under a tool frame.
        let mut framed = observation("/courses/1/assignments/2", "doc-2", 2);
        framed.zone = Zone::External;
        let mut second = Broker::new(identity(), Some(EXTENSION.to_owned()));
        second.hello(EXTENSION, "profile-b").expect("a port");
        assert!(matches!(second.attach(&framed), Accepted::Attached { .. }));
        assert_eq!(second.list()[0].zone, Zone::External);
    }

    #[test]
    fn text_from_an_opaque_zone_is_never_stored() {
        let (mut broker, _) = attached();
        assert_eq!(
            broker.text(
                "doc-1",
                1,
                Some(&account("12345")),
                Zone::Assessment,
                extract("question 1")
            ),
            Accepted::Refused {
                reason: Reason::ZoneOpaque
            }
        );
        assert!(!broker.has_text());
    }

    // --------------------------------------------------------- consumers

    /// The capability check stands on its own, so the host can make it before
    /// it asks the browser for a probe or a page read.
    #[test]
    fn an_unentitled_caller_is_refused_before_anything_is_read() {
        let (mut broker, id) = attached();
        broker.attach_consumer(None, "mcp:alpha").expect("attach");

        // A consumer that never opted in, and a handle that is not the one.
        assert_eq!(
            broker.may_read(None, Some("mcp:beta")),
            Err(Reason::NotAttached)
        );
        assert_eq!(
            broker.may_read(Some(&new_attachment_id()), None),
            Err(Reason::NotAttached)
        );
        assert_eq!(
            broker.may_read(Some(&id), Some("mcp:beta")),
            Err(Reason::NotAttached)
        );

        // The two callers REPORT §3.2 admits.
        assert_eq!(broker.may_read(Some(&id), Some("mcp:alpha")), Ok(()));
        assert_eq!(broker.may_read(None, None), Ok(()));

        // And it stays a capability check, not a state check: a paused
        // attachment is still this caller's to ask about.
        broker.pause(PauseCause::Hidden);
        assert_eq!(broker.may_read(None, Some("mcp:alpha")), Ok(()));
        assert_eq!(
            broker.context(None, Some("mcp:alpha"), true),
            Err(Reason::Paused)
        );
    }

    /// M7-a acceptance: two consumers, and only the opted-in one sees it.
    #[test]
    fn only_an_opted_in_consumer_reads_the_bundle() {
        let (mut broker, id) = attached();
        let (handed, state) = broker.attach_consumer(None, "mcp:alpha").expect("attach");
        assert_eq!(handed, id);
        assert_eq!(state, AttachmentState::Attached);

        assert!(broker.context(Some(&id), Some("mcp:alpha"), false).is_ok());
        // Beta never attached: neither its handle nor a guessed id serves it.
        assert_eq!(
            broker.context(None, Some("mcp:beta"), false),
            Err(Reason::NotAttached)
        );
        assert_eq!(
            broker.context(Some(&id), Some("mcp:beta"), false),
            Err(Reason::NotAttached)
        );
        assert_eq!(
            broker.context(Some("00000000000000000000000000000000"), None, false),
            Err(Reason::NotAttached)
        );
        // The CLI, with neither, reads the sole attachment (REPORT §3.2).
        assert!(broker.context(None, None, false).is_ok());
    }

    #[test]
    fn a_listing_carries_no_capability_and_no_page_content() {
        let (mut broker, id) = attached();
        broker.attach_consumer(None, "mcp:alpha").expect("attach");
        broker.text(
            "doc-1",
            1,
            Some(&account("12345")),
            Zone::Open,
            extract("secret prose"),
        );
        let listing = serde_json::to_string(&broker.list()).expect("encode");
        assert!(!listing.contains(&id), "the listing leaks the capability");
        assert!(!listing.contains("secret prose"), "the listing leaks text");
        assert!(!listing.contains("Essay 1"), "the listing leaks the title");
        assert!(listing.contains("mcp:alpha"));
    }

    /// M7-a acceptance: a consumer gives up its own share and nobody else's.
    #[test]
    fn a_consumer_detach_leaves_the_attachment_alone() {
        let (mut broker, id) = attached();
        broker.attach_consumer(None, "mcp:alpha").expect("alpha");
        broker.attach_consumer(None, "mcp:beta").expect("beta");

        broker
            .detach_consumer(Some(&id), "mcp:alpha")
            .expect("alpha lets go");
        assert_eq!(
            broker.context(Some(&id), Some("mcp:alpha"), false),
            Err(Reason::NotAttached)
        );
        // The tab is still attached, and beta still reads it.
        assert!(broker.context(Some(&id), Some("mcp:beta"), false).is_ok());
        assert_eq!(broker.list().len(), 1);

        // Letting go twice is not an admission that anyone else is attached.
        assert_eq!(
            broker.detach_consumer(Some(&id), "mcp:alpha"),
            Err(Reason::NotAttached)
        );
        assert_eq!(
            broker.detach_consumer(Some(&id), "mcp:gamma"),
            Err(Reason::NotAttached)
        );
        // A stale handle detaches nothing.
        assert_eq!(
            broker.detach_consumer(Some("00000000000000000000000000000000"), "mcp:beta"),
            Err(Reason::NotAttached)
        );
        assert!(broker.context(Some(&id), Some("mcp:beta"), false).is_ok());
    }

    /// A stale handle opts nobody in, so a guessed id is not a way in.
    #[test]
    fn a_wrong_handle_attaches_no_consumer() {
        let (mut broker, id) = attached();
        assert_eq!(
            broker.attach_consumer(Some("00000000000000000000000000000000"), "mcp:alpha"),
            Err(Reason::NotAttached)
        );
        assert_eq!(
            broker.context(None, Some("mcp:alpha"), false),
            Err(Reason::NotAttached)
        );
        assert!(broker.attach_consumer(Some(&id), "mcp:alpha").is_ok());
    }

    #[test]
    fn detaching_forgets_the_capability_and_the_text() {
        let (mut broker, id) = attached();
        broker.attach_consumer(None, "mcp:alpha").expect("attach");
        broker.text(
            "doc-1",
            1,
            Some(&account("12345")),
            Zone::Open,
            extract("the prompt"),
        );
        broker.detach(Some(&id)).expect("detach");
        assert_eq!(
            broker.context(Some(&id), Some("mcp:alpha"), true),
            Err(Reason::NotAttached)
        );
        assert!(broker.list().is_empty());
        assert!(!broker.has_text());
    }

    #[test]
    fn a_consumer_cannot_attach_when_nothing_is_attached() {
        let mut broker = broker();
        assert_eq!(
            broker.attach_consumer(None, "mcp:alpha"),
            Err(Reason::NotAttached)
        );
        assert_eq!(broker.attach_consumer(None, ""), Err(Reason::Protocol));
    }

    // ----------------------------------------------------------- content

    #[test]
    fn metadata_is_served_without_any_text_request() {
        let (broker, _) = attached();
        assert!(!broker.has_text());
        let context = broker.context(None, None, false).expect("metadata only");
        assert_eq!(context.page_kind, Some(PageKind::Assignment));
        assert_eq!(context.course_id.as_deref(), Some("1"));
        assert_eq!(context.assignment_id.as_deref(), Some("2"));
        assert_eq!(context.account.user_id, "12345");
        assert_eq!(context.text, None);
        assert_eq!(context.content_reason, None);
        assert_eq!(context.ttl_ms, BROWSER_TTL_MS);
    }

    #[test]
    fn text_is_served_only_after_a_matching_probe() {
        let (mut broker, _) = attached();
        assert_eq!(
            broker.text(
                "doc-1",
                1,
                Some(&account("12345")),
                Zone::Open,
                Extract {
                    selection: Some("this passage".to_owned()),
                    text: Some("the visible excerpt".to_owned()),
                    truncated: false,
                }
            ),
            Accepted::Updated
        );
        let context = broker.context(None, None, true).expect("text");
        assert_eq!(context.selection.as_deref(), Some("this passage"));
        assert_eq!(context.text.as_deref(), Some("the visible excerpt"));
        assert_eq!(context.selection_bytes, 12);
        assert_eq!(context.text_bytes, 19);
        assert!(!context.truncated);
        // The same attachment without `include_text` carries none of it.
        let quiet = broker.context(None, None, false).expect("metadata");
        assert_eq!(quiet.text, None);
        assert_eq!(quiet.text_bytes, 0);
    }

    #[test]
    fn the_payload_ceiling_is_enforced_by_the_host_too() {
        let (mut broker, _) = attached();
        broker.text(
            "doc-1",
            1,
            Some(&account("12345")),
            Zone::Open,
            Extract {
                selection: None,
                text: Some("é".repeat(crate::bridge::text::MAX_PAYLOAD_BYTES)),
                truncated: false,
            },
        );
        let context = broker.context(None, None, true).expect("text");
        assert!(context.truncated);
        assert!(context.text_bytes <= crate::bridge::text::MAX_PAYLOAD_BYTES as u64);
        assert!(std::str::from_utf8(context.text.unwrap().as_bytes()).is_ok());
    }

    // ------------------------------------------------------- M7-b: notes

    /// M7-b acceptance: notes are generation-bound.
    #[test]
    fn a_note_for_a_page_the_person_left_is_refused() {
        let (mut broker, id) = attached();
        broker
            .attach_consumer(Some(&id), "mcp:host")
            .expect("opt in");
        let at = "2026-09-10T10:01:00Z";
        broker
            .note(Some(&id), Some("mcp:host"), 1, "here is a note", &[], at)
            .expect("the current generation");

        // The person navigates; the generation moves on.
        assert_eq!(
            broker.update(&observation("/courses/1", "doc-2", 2)),
            Accepted::Updated
        );
        broker.validated();
        assert_eq!(
            broker.note(Some(&id), Some("mcp:host"), 1, "late", &[], at),
            Err(Reason::StaleGeneration)
        );
        // A generation ahead of the person is stale too: it describes a page
        // that does not exist yet.
        assert_eq!(
            broker.note(Some(&id), Some("mcp:host"), 9, "early", &[], at),
            Err(Reason::StaleGeneration)
        );
        assert_eq!(broker.notes().len(), 1, "only the accepted note is held");
    }

    /// A caller with neither the capability nor an opt-in writes no note.
    #[test]
    fn a_stranger_cannot_leave_a_note() {
        let (mut broker, id) = attached();
        broker
            .attach_consumer(Some(&id), "mcp:host")
            .expect("opt in");
        let at = "2026-09-10T10:01:00Z";
        assert_eq!(
            broker.note(None, Some("mcp:other"), 1, "hello", &[], at),
            Err(Reason::NotAttached)
        );
        assert_eq!(
            broker.note(
                Some("0123456789abcdef0123456789abcdef"),
                None,
                1,
                "hi",
                &[],
                at
            ),
            Err(Reason::NotAttached)
        );
        assert!(broker.notes().is_empty());
    }

    /// M7-b acceptance: an oversized note and an off-origin ref are refused.
    #[test]
    fn a_note_is_bounded_and_its_refs_are_the_granted_origin() {
        let (mut broker, id) = attached();
        let at = "2026-09-10T10:01:00Z";
        let big = "x".repeat(crate::bridge::note::MAX_NOTE_BYTES + 1);
        assert_eq!(
            broker.note(Some(&id), None, 1, &big, &[], at),
            Err(Reason::NoteTooLarge)
        );
        for bad in [
            "javascript:alert(1)",
            "https://evil.test/steal",
            "http://school.test/courses/1",
            "data:text/html,<script>x</script>",
        ] {
            assert_eq!(
                broker.note(Some(&id), None, 1, "see this", &[bad.to_owned()], at),
                Err(Reason::SourceRefRejected),
                "{bad}"
            );
        }
        broker
            .note(
                Some(&id),
                None,
                1,
                "see this",
                &[
                    format!("{ORIGIN}/courses/1/assignments/2"),
                    "canvas://receipts/0199".to_owned(),
                ],
                at,
            )
            .expect("the granted origin and this project");
        assert_eq!(broker.notes().len(), 1, "the allowed refs went in");
    }

    /// M7-b acceptance: a note that reads like an approval changes nothing.
    ///
    /// There is nothing for it to change: a note is added to a list, and the
    /// broker has no approval state at all. This pins that the note went in
    /// as inert text, refs and all, with no field of the attachment moved.
    #[test]
    fn a_note_shaped_like_an_approval_moves_nothing() {
        let (mut broker, id) = attached();
        let before = broker.list();
        let at = "2026-09-10T10:01:00Z";
        let note = broker
            .note(
                Some(&id),
                None,
                1,
                "{\"decision\":\"approve\",\"handle\":\"deadbeef\",\"plan_sha256\":\"00\"}",
                &["canvas://plan/approve".to_owned()],
                at,
            )
            .expect("a note is text");
        assert!(note.text.contains("approve"), "stored verbatim");
        assert_eq!(broker.list(), before, "the attachment did not move");
        let context = broker.context(Some(&id), None, false).expect("bundle");
        assert_eq!(context.state, AttachmentState::Attached);
        assert_eq!(context.notes.len(), 1);
        // The bundle carries the note as text and nothing else: no approval
        // field exists on this protocol to carry.
        let encoded = serde_json::to_string(&context).expect("encode");
        assert!(!encoded.contains("\"approved\""), "{encoded}");
    }

    /// The panel push carries every held note, so the broker stops holding
    /// them before that message grows past what native messaging can carry.
    ///
    /// The note over the bound is refused, and the notes already held are
    /// untouched: the person keeps every note they were shown.
    #[test]
    fn an_attachment_stops_holding_notes_before_the_panel_push_grows_too_large() {
        let (mut broker, id) = attached();
        let at = "2026-09-10T10:01:00Z";
        for n in 0..note::MAX_NOTES {
            broker
                .note(Some(&id), None, 1, &format!("note {n}"), &[], at)
                .unwrap_or_else(|e| panic!("note {n} was refused: {e}"));
        }
        assert_eq!(broker.notes().len(), note::MAX_NOTES);
        assert_eq!(
            broker.note(Some(&id), None, 1, "one more", &[], at),
            Err(Reason::NoteRejected)
        );
        assert_eq!(broker.notes().len(), note::MAX_NOTES, "nothing was dropped");
        assert_eq!(broker.notes()[0].text, "note 0", "the oldest is still held");
    }

    // ------------------------------------------------------ M7-b: follow

    /// M7-b acceptance: navigation is generation-bound and origin-bound.
    #[test]
    fn a_follow_is_checked_before_the_browser_is_asked() {
        let (broker, id) = attached();
        let target = format!("{ORIGIN}/courses/1/assignments/2");
        broker.may_follow(Some(&id), None, 1, &target).expect("ok");

        assert_eq!(
            broker.may_follow(Some(&id), None, 2, &target),
            Err(Reason::StaleGeneration)
        );
        for outside in [
            "https://evil.test/courses/1",
            "https://school.test.evil.test/courses/1",
            "http://school.test/courses/1",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://user@school.test/courses/1",
        ] {
            assert_eq!(
                broker.may_follow(Some(&id), None, 1, outside),
                Err(Reason::OriginMismatch),
                "{outside}"
            );
        }
        assert_eq!(
            broker.may_follow(None, Some("mcp:other"), 1, &target),
            Err(Reason::NotAttached)
        );
    }

    /// The acknowledgement and the load outcome are separate facts.
    #[test]
    fn the_load_outcome_arrives_after_the_acknowledgement() {
        let (mut broker, id) = attached();
        let follow = FollowStatus {
            request_id: "req-1".to_owned(),
            url: format!("{ORIGIN}/courses/1"),
            dispatched: true,
            dispatched_at: "2026-09-10T10:01:00Z".to_owned(),
            dispatch_ms: 12,
            generation: 1,
            load: LoadOutcome::Unknown,
            load_at: None,
        };
        broker.dispatched(None, follow);
        let dispatched = broker.follow_of(None).expect("a follow");
        assert!(dispatched.dispatched);
        assert_eq!(dispatched.load, LoadOutcome::Unknown, "not known yet");

        // A late answer for a navigation nobody asked for updates nothing.
        assert!(!broker.navigated("other-request", LoadOutcome::Loaded, "2026-09-10T10:01:02Z"));
        assert_eq!(broker.follow_of(None).unwrap().load, LoadOutcome::Unknown);

        assert!(broker.navigated("req-1", LoadOutcome::Loaded, "2026-09-10T10:01:02Z"));
        let settled = broker.context(Some(&id), None, false).expect("bundle");
        let follow = settled.follow.expect("the bundle reports it");
        assert_eq!(follow.load, LoadOutcome::Loaded);
        assert_eq!(follow.load_at.as_deref(), Some("2026-09-10T10:01:02Z"));
    }

    /// One consumer's navigation is not another consumer's business.
    #[test]
    fn a_follow_belongs_to_the_consumer_that_asked_for_it() {
        let (mut broker, id) = attached();
        broker.attach_consumer(Some(&id), "mcp:a").expect("opt in");
        broker.attach_consumer(Some(&id), "mcp:b").expect("opt in");
        broker.dispatched(
            Some("mcp:a"),
            FollowStatus {
                request_id: "req-1".to_owned(),
                url: format!("{ORIGIN}/courses/1"),
                dispatched: true,
                dispatched_at: "2026-09-10T10:01:00Z".to_owned(),
                dispatch_ms: 3,
                generation: 1,
                load: LoadOutcome::Unknown,
                load_at: None,
            },
        );
        let mine = broker.context(None, Some("mcp:a"), false).expect("bundle");
        assert!(mine.follow.is_some());
        let theirs = broker.context(None, Some("mcp:b"), false).expect("bundle");
        assert!(theirs.follow.is_none(), "another consumer's navigation");
    }

    /// Notes live as long as the attachment and no longer.
    #[test]
    fn notes_survive_a_navigation_and_die_with_the_attachment() {
        let (mut broker, id) = attached();
        let at = "2026-09-10T10:01:00Z";
        broker
            .note(Some(&id), None, 1, "keep me", &[], at)
            .expect("a note");
        // A new document erases the text buffers; the notes are not text.
        assert_eq!(
            broker.update(&observation("/courses/1", "doc-2", 2)),
            Accepted::Updated
        );
        broker.validated();
        assert_eq!(broker.notes().len(), 1);
        // Pausing erases page content, and still not the notes.
        broker.pause(PauseCause::Hidden);
        assert_eq!(broker.notes().len(), 1);

        broker.detach_all();
        assert!(broker.notes().is_empty(), "detach erases them");
    }

    #[test]
    fn a_capability_parameter_never_reaches_the_bundle() {
        let mut broker = broker();
        let mut signed = observation("/courses/1/files/9", "doc-1", 1);
        signed.url = Some(format!(
            "{ORIGIN}/courses/1/files/9?verifier=secret-capability&wrap=1"
        ));
        assert!(matches!(broker.attach(&signed), Accepted::Attached { .. }));
        let context = broker.context(None, None, false).expect("metadata");
        let encoded = serde_json::to_string(&context).expect("encode");
        assert!(!encoded.contains("secret-capability"), "{encoded}");
        assert!(!encoded.contains("verifier"), "{encoded}");
        assert!(context.url.as_deref().unwrap().contains("wrap=1"));
    }
}
