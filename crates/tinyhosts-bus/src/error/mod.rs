//! Crate-wide error and result types.
//!
//! Every fallible public function in this crate returns [`Result`], and every
//! failure mode is a distinct [`Error`] variant. Add a variant rather than
//! encoding new context into an existing message: callers match on variants,
//! and message text is not a stable API.
//!
//! Variants carry the data a caller needs to react, keep their `#[error]`
//! message lowercase and free of trailing punctuation, and are documented so
//! the rendered rustdoc explains when each one occurs.
//!
//! Provider failures name the provider they came from. A run may talk to more
//! than one host in the same process, and "unauthorized" is not actionable
//! until the reader knows which credential was rejected.

/// Errors returned by this crate.
#[derive(thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// Authorized preparation named an invalid or out-of-scope directory.
    #[error("invalid preparation directory: {reason}")]
    PreparationPath {
        /// Validation failure, without file contents.
        reason: String,
    },
    /// Source collection exceeded its configured bounded budget.
    #[error("directory preparation exceeds {limit}")]
    PreparationLimit {
        /// The exhausted budget name.
        limit: String,
    },
    /// An API key was empty or contained only whitespace.
    #[error("api key must not be empty")]
    EmptyApiKey,

    /// No API key was found in the environment for a provider.
    ///
    /// `variables` lists every name that was searched, in order.
    #[error("no api key for {provider}: set one of {variables}")]
    MissingApiKey {
        /// The provider whose credential is missing.
        provider: String,
        /// The environment variable names that were searched, comma separated.
        variables: String,
    },

    /// A site name was empty or contained only whitespace.
    #[error("site name must not be empty")]
    EmptySiteName,

    /// A site name, deployment id or other identifier that goes into a
    /// provider URL path was empty, `.` or `..`, which would address a
    /// different endpoint than the one named.
    #[error("identifier must not be empty, `.` or `..`")]
    InvalidIdentifier,

    /// A deployment was requested with no files in it.
    #[error("deployment bundle contains no files")]
    EmptyBundle,

    /// A bundle entry named a path that cannot be deployed.
    ///
    /// Bundle paths are relative, slash separated, and may not traverse out of
    /// the bundle root.
    #[error("bundle path {path} is not a relative path inside the bundle")]
    InvalidBundlePath {
        /// The rejected path, as it was supplied.
        path: String,
    },

    /// A structurally valid bundle path referred to credential material.
    #[error("bundle contains credential material")]
    CredentialBundlePath {
        /// The refused path; never included in Display or Debug.
        path: String,
    },

    /// The filesystem refused a read while building a bundle from a directory.
    #[error("cannot read bundle entry")]
    ReadBundle {
        /// The path that could not be read.
        path: String,
        /// The operating system's reason.
        reason: String,
    },

    /// An environment variable name was empty or contained only whitespace.
    #[error("environment variable name must not be empty")]
    EmptyEnvKey,

    /// A domain name was empty or contained only whitespace.
    #[error("domain name must not be empty")]
    EmptyDomain,

    /// An analytics window ended at or before it started.
    #[error("analytics window must end after it starts")]
    InvalidAnalyticsWindow,

    /// The request never reached the provider, or its response never arrived.
    #[error("request to {provider} failed")]
    Transport {
        /// The provider that was being called.
        provider: String,
        /// The transport-level reason.
        reason: String,
    },

    /// The provider rejected the credential.
    #[error("{provider} rejected the api key")]
    Unauthorized {
        /// The provider that rejected it.
        provider: String,
    },

    /// The credential is valid but not permitted to touch the resource.
    #[error("{provider} denied access to {resource}")]
    Forbidden {
        /// The provider that denied the request.
        provider: String,
        /// The resource that was denied, as this crate named it.
        resource: String,
    },

    /// The provider has no such resource.
    #[error("{resource} not found on {provider}")]
    NotFound {
        /// The provider that was queried.
        provider: String,
        /// The resource that was missing.
        resource: String,
    },

    /// The provider is throttling this credential.
    #[error("{provider} rate limited the request")]
    RateLimited {
        /// The provider that throttled the request.
        provider: String,
    },

    /// The provider returned a failure this crate has no specific variant for.
    #[error("{provider} returned {status} for {resource}: {message}")]
    Api {
        /// The provider that failed.
        provider: String,
        /// The HTTP status code.
        status: u16,
        /// The resource that was being fetched, as this crate named it.
        resource: String,
        /// The provider's own message, or its status text.
        message: String,
    },

    /// A provider response did not match the shape this crate expects.
    #[error("cannot decode the {provider} response for {resource}: {reason}")]
    Decode {
        /// The provider whose response could not be read.
        provider: String,
        /// The resource that was being fetched.
        resource: String,
        /// The deserialization error.
        reason: String,
    },

    /// An alternate API root was given over plain HTTP to a non-loopback host.
    ///
    /// the implementation provider connector sends the account's bearer
    /// credential to every request `base_url` produces, so an `http://` root
    /// reaching outside the local machine would carry it in cleartext.
    /// `https://` is always accepted; `http://` is accepted only against
    /// `localhost`, `127.0.0.1`, or `::1`, which is what a test suite or a
    /// local mock needs.
    #[error("base url must be https, or http against a loopback host")]
    InsecureBaseUrl {
        /// The rejected root, as it was supplied.
        base_url: String,
    },

    /// A provider was named that this build does not have.
    ///
    /// Either the name is not a provider, or its Cargo feature is off.
    #[error("unknown hosting provider {name}")]
    UnknownProvider {
        /// The name that was supplied.
        name: String,
    },

    /// The provider cannot do something the unified API exposes.
    ///
    /// Not every host has every capability, and a stub that silently succeeds
    /// is worse than a refusal that names what is missing.
    #[error("{provider} cannot {capability}")]
    Unsupported {
        /// The provider that lacks the capability.
        provider: String,
        /// The capability, phrased to complete the sentence "cannot ...".
        capability: String,
    },

    /// No installed integration on the account can provision this database.
    ///
    /// On Vercel a managed database comes from a marketplace integration, so
    /// one has to be installed on the account before a store can be created.
    #[error("no installed {provider} integration provides a {kind} database")]
    NoDatabaseProduct {
        /// The provider that was searched.
        provider: String,
        /// The requested database kind.
        kind: String,
    },

    /// A database was created but did not come up.
    #[error("database {name} was provisioned but reported status {status}")]
    DatabaseNotReady {
        /// The database's name.
        name: String,
        /// The status the provider reported.
        status: String,
    },

    /// A JSON envelope crossing a process boundary could not be read.
    #[error("cannot decode the request envelope")]
    Envelope {
        /// The deserialization error.
        reason: String,
    },
}

// Match Display's redaction: Debug must not expose rejected URLs, parser input
// or operating-system paths through the retained compatibility fields.
impl std::fmt::Debug for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

/// The crate's standard result type.
///
/// Use this alias in public signatures instead of spelling out
/// `std::result::Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;
