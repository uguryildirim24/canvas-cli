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

use crate::bridge::ipc::{AttachmentState, AttachmentSummary, Context, Reason, VerifiedAccount};
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

    /// Opt one consumer in and hand it the capability.
    pub fn attach_consumer(&mut self, consumer: &str) -> Result<(String, AttachmentState), Reason> {
        if consumer.is_empty() {
            return Err(Reason::Protocol);
        }
        let Some(current) = self.attachment.as_mut() else {
            return Err(Reason::NotAttached);
        };
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

    /// Resolve a `here` request to the browser section of the bundle.
    ///
    /// `attachment_id` is the capability. `consumer`, which the adapter sets
    /// and a model cannot, selects the attachment that consumer opted into.
    /// With neither — the CLI — the sole attachment is served, which is what
    /// REPORT §3.2 permits the CLI and nothing else.
    pub fn context(
        &self,
        attachment_id: Option<&str>,
        consumer: Option<&str>,
        include_text: bool,
    ) -> Result<Context, Reason> {
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
        })
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
    fn zone_of(observation: &Observation) -> Zone {
        let from_route = observation
            .url
            .as_deref()
            .and_then(sanitize_url)
            .and_then(|url| reqwest::Url::parse(&url).ok())
            .map_or(Zone::Unknown, |url| {
                crate::bridge::wire::zone_for_route(&classify_route(url.path()))
            });
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

    /// M7-a acceptance: two consumers, and only the opted-in one sees it.
    #[test]
    fn only_an_opted_in_consumer_reads_the_bundle() {
        let (mut broker, id) = attached();
        let (handed, state) = broker.attach_consumer("mcp:alpha").expect("attach");
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
        broker.attach_consumer("mcp:alpha").expect("attach");
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

    #[test]
    fn detaching_forgets_the_capability_and_the_text() {
        let (mut broker, id) = attached();
        broker.attach_consumer("mcp:alpha").expect("attach");
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
            broker.attach_consumer("mcp:alpha"),
            Err(Reason::NotAttached)
        );
        assert_eq!(broker.attach_consumer(""), Err(Reason::Protocol));
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
