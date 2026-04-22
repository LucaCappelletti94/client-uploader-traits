#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(
    clippy::all,
    clippy::cargo,
    clippy::pedantic,
    clippy::expect_used,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unwrap_used
)]
#![allow(clippy::module_name_repetitions)]

use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt;
use std::future::Future;
use std::path::Path;
use std::time::Duration;

use url::Url;

/// Marker used when a service creates new resources without a caller-supplied target identifier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NoCreateTarget;

/// Shared classification of upload input sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UploadSourceKind {
    /// Upload bytes come from a local file path.
    Path,
    /// Upload bytes come from a reader or stream-like source.
    Reader,
    /// Upload bytes are already materialized in memory.
    Bytes,
}

/// Normalized draft-file reconciliation semantics shared by Zenodo and Figshare.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DraftFilePolicyKind {
    /// Remove all existing files before publishing the new upload set.
    ReplaceAll,
    /// Replace only the files whose names match one of the new uploads.
    UpsertByFilename,
    /// Keep existing files and add the new uploads alongside them.
    KeepExistingAndAdd,
}

/// Normalized per-file conflict semantics shared by direct-upload services such as Internet Archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExistingFileConflictPolicyKind {
    /// Fail when a file with the same name already exists.
    Error,
    /// Skip uploads whose names already exist.
    Skip,
    /// Replace the existing file in place.
    Overwrite,
    /// Replace the existing file while preserving history on the remote service.
    OverwriteKeepingHistory,
}

/// Generic request for “create a brand-new published resource”.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatePublicationRequest<Target, Metadata, Upload> {
    /// Caller-supplied creation target.
    ///
    /// Use [`NoCreateTarget`] for services that create new resources without an
    /// external identifier.
    pub target: Target,
    /// Descriptive metadata to apply before publication.
    pub metadata: Metadata,
    /// Files to upload as part of the publication workflow.
    pub uploads: Vec<Upload>,
}

impl<Target, Metadata, Upload> CreatePublicationRequest<Target, Metadata, Upload> {
    /// Creates a new request from explicit parts.
    #[must_use]
    pub fn new(target: Target, metadata: Metadata, uploads: Vec<Upload>) -> Self {
        Self {
            target,
            metadata,
            uploads,
        }
    }

    /// Appends one upload to the request.
    #[must_use]
    pub fn with_upload(mut self, upload: Upload) -> Self {
        self.uploads.push(upload);
        self
    }

    /// Maps the upload list into another upload type while preserving target and metadata.
    #[must_use]
    pub fn map_uploads<MappedUpload>(
        self,
        mut map: impl FnMut(Upload) -> MappedUpload,
    ) -> CreatePublicationRequest<Target, Metadata, MappedUpload> {
        CreatePublicationRequest {
            target: self.target,
            metadata: self.metadata,
            uploads: self.uploads.into_iter().map(&mut map).collect(),
        }
    }
}

impl<Metadata, Upload> CreatePublicationRequest<NoCreateTarget, Metadata, Upload> {
    /// Creates a new request for services that do not require a caller-supplied creation target.
    #[must_use]
    pub fn untargeted(metadata: Metadata, uploads: Vec<Upload>) -> Self {
        Self::new(NoCreateTarget, metadata, uploads)
    }
}

/// Generic request for “update or upsert an existing published resource”.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdatePublicationRequest<ResourceId, Metadata, Policy, Upload> {
    /// Identifier of the existing resource family or mutable root object.
    pub resource_id: ResourceId,
    /// Descriptive metadata to apply during the workflow.
    pub metadata: Metadata,
    /// Upload reconciliation or conflict policy.
    pub policy: Policy,
    /// Files to upload as part of the workflow.
    pub uploads: Vec<Upload>,
}

impl<ResourceId, Metadata, Policy, Upload>
    UpdatePublicationRequest<ResourceId, Metadata, Policy, Upload>
{
    /// Creates a new request from explicit parts.
    #[must_use]
    pub fn new(
        resource_id: ResourceId,
        metadata: Metadata,
        policy: Policy,
        uploads: Vec<Upload>,
    ) -> Self {
        Self {
            resource_id,
            metadata,
            policy,
            uploads,
        }
    }

    /// Appends one upload to the request.
    #[must_use]
    pub fn with_upload(mut self, upload: Upload) -> Self {
        self.uploads.push(upload);
        self
    }

    /// Maps the upload list into another upload type while preserving identifiers, metadata, and policy.
    #[must_use]
    pub fn map_uploads<MappedUpload>(
        self,
        mut map: impl FnMut(Upload) -> MappedUpload,
    ) -> UpdatePublicationRequest<ResourceId, Metadata, Policy, MappedUpload> {
        UpdatePublicationRequest {
            resource_id: self.resource_id,
            metadata: self.metadata,
            policy: self.policy,
            uploads: self.uploads.into_iter().map(&mut map).collect(),
        }
    }
}

/// Validation error returned by shared upload-name helpers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadNameValidationError {
    /// One upload had an empty remote filename.
    EmptyFilename,
    /// More than one upload targeted the same remote filename.
    DuplicateFilename {
        /// The duplicated filename.
        filename: String,
    },
}

impl fmt::Display for UploadNameValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFilename => f.write_str("upload filename cannot be empty"),
            Self::DuplicateFilename { filename } => {
                write!(f, "duplicate upload filename: {filename}")
            }
        }
    }
}

impl StdError for UploadNameValidationError {}

/// Minimal configuration surface shared by the uploader clients.
pub trait ClientContext {
    /// Service-specific endpoint configuration type.
    type Endpoint;
    /// Service-specific polling configuration type.
    type PollOptions;
    /// Service-specific error type.
    type Error;

    /// Returns the configured endpoint roots.
    fn endpoint(&self) -> &Self::Endpoint;

    /// Returns the configured polling behavior.
    fn poll_options(&self) -> &Self::PollOptions;

    /// Returns the overall request timeout, when configured.
    fn request_timeout(&self) -> Option<Duration>;

    /// Returns the TCP connect timeout, when configured.
    fn connect_timeout(&self) -> Option<Duration>;
}

/// Shared auth-state inspection surface.
pub trait MaybeAuthenticatedClient: ClientContext {
    /// Returns whether the client currently has credentials available for authenticated calls.
    fn has_auth(&self) -> bool;
}

/// Shared inspection surface for upload specifications.
pub trait UploadSpecLike {
    /// Returns the remote filename that the upload will expose.
    fn filename(&self) -> &str;

    /// Returns the kind of source backing this upload.
    fn source_kind(&self) -> UploadSourceKind;

    /// Returns the exact content length when it is already known cheaply.
    fn content_length(&self) -> Option<u64> {
        None
    }

    /// Returns the HTTP content type when the upload tracks one.
    fn content_type(&self) -> Option<&str> {
        None
    }
}

/// Shared inspection surface for remote file entries.
pub trait RepositoryFile {
    /// Service-specific file identifier type.
    type Id: Clone;

    /// Returns the file identifier when the service exposes one.
    fn file_id(&self) -> Option<Self::Id>;

    /// Returns the remote filename or key.
    fn file_name(&self) -> &str;

    /// Returns the file size in bytes when known.
    fn size_bytes(&self) -> Option<u64>;

    /// Returns a checksum string when the service reports one.
    fn checksum(&self) -> Option<&str> {
        None
    }

    /// Returns the best direct download URL when one is already present on the payload.
    fn download_url(&self) -> Option<&Url> {
        None
    }
}

/// Shared inspection surface for top-level resource payloads.
///
/// This trait intentionally treats embedded file lists as advisory. Some
/// services, notably Figshare, may return partial embedded file lists on the
/// main resource payload. Use [`ListResourceFiles`] when complete enumeration
/// matters.
pub trait RepositoryRecord {
    /// Service-specific resource identifier type.
    type Id: Clone;
    /// Service-specific file entry type.
    type File: RepositoryFile;

    /// Returns the top-level resource identifier when it can be read directly from the payload.
    fn resource_id(&self) -> Option<Self::Id>;

    /// Returns a display title when one is available.
    fn title(&self) -> Option<&str>;

    /// Returns the embedded file list on the payload.
    fn files(&self) -> &[Self::File];
}

/// Shared inspection surface for DOI-backed resources.
pub trait DoiBackedRecord {
    /// Service-specific DOI type.
    type Doi: Clone;

    /// Returns the resource DOI when one is present.
    fn doi(&self) -> Option<Self::Doi>;
}

/// Shared inspection surface for mutable draft-like resources.
pub trait DraftResource {
    /// Service-specific draft identifier type.
    type Id: Clone;
    /// Service-specific draft file entry type.
    type File: RepositoryFile;

    /// Returns the draft identifier.
    fn draft_id(&self) -> Self::Id;

    /// Returns the embedded draft file list.
    fn files(&self) -> &[Self::File];
}

/// Shared inspection surface for publication workflow results.
pub trait PublicationOutcome {
    /// Public resource type returned by the workflow.
    type PublicResource;

    /// Returns the final public resource visible after the workflow completes.
    fn public_resource(&self) -> &Self::PublicResource;

    /// Returns whether the workflow definitely created a brand-new resource.
    ///
    /// `None` means the workflow result does not carry this distinction.
    fn created(&self) -> Option<bool> {
        None
    }
}

/// Shared inspection surface for publication workflows that also return a mutable or private-side resource.
pub trait MutablePublicationOutcome: PublicationOutcome {
    /// Mutable or private-side resource type returned by the workflow.
    type MutableResource;

    /// Returns the mutable or private-side resource when the workflow exposes one.
    fn mutable_resource(&self) -> Option<&Self::MutableResource>;
}

/// Shared publication-state inspection for mutable resources.
pub trait DraftState {
    /// Returns whether the remote object is already published.
    fn is_published(&self) -> bool;

    /// Returns whether metadata edits are currently allowed.
    fn allows_metadata_updates(&self) -> bool;
}

/// Capability for reading one public resource by its primary service identifier.
pub trait ReadPublicResource: ClientContext {
    /// Service-specific resource identifier type.
    type ResourceId;
    /// Returned public resource payload.
    type Resource;

    /// Fetches one public resource.
    fn get_public_resource(
        &self,
        id: &Self::ResourceId,
    ) -> impl Future<Output = Result<Self::Resource, Self::Error>>;
}

/// Capability for searching public resources.
pub trait SearchPublicResources: ClientContext {
    /// Service-specific query type.
    type Query;
    /// Service-specific search result shape.
    type SearchResults;

    /// Executes a public search request.
    fn search_public_resources(
        &self,
        query: &Self::Query,
    ) -> impl Future<Output = Result<Self::SearchResults, Self::Error>>;
}

/// Shared inspection surface for search result containers.
pub trait SearchResultsLike {
    /// One result item.
    type Item;

    /// Returns the items materialized on the current result page.
    fn items(&self) -> &[Self::Item];

    /// Returns the total hit count when the backend reports one separately from the current page.
    fn total_hits(&self) -> Option<u64> {
        None
    }

    /// Returns the number of items on the current page.
    #[must_use]
    fn page_len(&self) -> usize {
        self.items().len()
    }

    /// Returns whether the current result page has no items.
    #[must_use]
    fn is_empty(&self) -> bool {
        self.items().is_empty()
    }
}

impl<T> SearchResultsLike for Vec<T> {
    type Item = T;

    fn items(&self) -> &[Self::Item] {
        self.as_slice()
    }
}

/// Capability for enumerating files attached to a public resource.
pub trait ListResourceFiles: ClientContext {
    /// Service-specific resource identifier type.
    type ResourceId;
    /// Service-specific file entry type.
    type File;

    /// Returns the complete visible file list for the resource.
    fn list_resource_files(
        &self,
        id: &Self::ResourceId,
    ) -> impl Future<Output = Result<Vec<Self::File>, Self::Error>>;
}

/// Capability for downloading one named public file to a local path.
pub trait DownloadNamedPublicFile: ClientContext {
    /// Service-specific resource identifier type.
    type ResourceId;
    /// Service-specific download result metadata.
    type Download;

    /// Downloads one named file from a public resource.
    fn download_named_public_file_to_path(
        &self,
        id: &Self::ResourceId,
        name: &str,
        path: &Path,
    ) -> impl Future<Output = Result<Self::Download, Self::Error>>;
}

/// Capability for creating and publishing a brand-new resource.
pub trait CreatePublication: ClientContext {
    /// Caller-supplied creation target type.
    type CreateTarget;
    /// Service-specific metadata type.
    type Metadata;
    /// Service-specific upload specification type.
    type Upload;
    /// Service-specific workflow result type.
    type Output;

    /// Runs the service’s “create a new published resource” workflow.
    fn create_publication(
        &self,
        request: CreatePublicationRequest<Self::CreateTarget, Self::Metadata, Self::Upload>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>>;
}

/// Capability for updating or upserting an existing published resource.
pub trait UpdatePublication: ClientContext {
    /// Service-specific resource identifier type.
    type ResourceId;
    /// Service-specific metadata type.
    type Metadata;
    /// Service-specific upload-policy type.
    type FilePolicy;
    /// Service-specific upload specification type.
    type Upload;
    /// Service-specific workflow result type.
    type Output;

    /// Runs the service’s “update or upsert an existing resource” workflow.
    fn update_publication(
        &self,
        request: UpdatePublicationRequest<
            Self::ResourceId,
            Self::Metadata,
            Self::FilePolicy,
            Self::Upload,
        >,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>>;
}

/// DOI-based lookup for public resources.
pub trait LookupByDoi: ClientContext {
    /// Service-specific DOI type.
    type Doi;
    /// Returned public resource payload.
    type Resource;

    /// Fetches one public resource by DOI.
    fn get_public_resource_by_doi(
        &self,
        doi: &Self::Doi,
    ) -> impl Future<Output = Result<Self::Resource, Self::Error>>;
}

/// Latest-version resolution for public resources that expose version families.
pub trait ResolveLatestPublicResource: ClientContext {
    /// Service-specific resource identifier type.
    type ResourceId;
    /// Returned public resource payload.
    type Resource;

    /// Resolves the latest public version in the resource family.
    fn resolve_latest_public_resource(
        &self,
        id: &Self::ResourceId,
    ) -> impl Future<Output = Result<Self::Resource, Self::Error>>;
}

/// Latest-version resolution for DOI-backed public resources.
pub trait ResolveLatestPublicResourceByDoi: ClientContext {
    /// Service-specific DOI type.
    type Doi;
    /// Returned public resource payload.
    type Resource;

    /// Resolves the latest public version in the DOI-backed resource family.
    fn resolve_latest_public_resource_by_doi(
        &self,
        doi: &Self::Doi,
    ) -> impl Future<Output = Result<Self::Resource, Self::Error>>;
}

/// Shared classification trait for Zenodo/Figshare draft reconciliation policies.
pub trait DraftFilePolicy {
    /// Returns the normalized reconciliation mode.
    fn kind(&self) -> DraftFilePolicyKind;
}

/// Shared classification trait for Internet Archive-style per-file conflict policies.
pub trait ExistingFileConflictPolicy {
    /// Returns the normalized conflict mode.
    fn kind(&self) -> ExistingFileConflictPolicyKind;
}

/// Lower-level mutable-draft workflow shared most directly by Zenodo and Figshare.
pub trait DraftWorkflow: ClientContext {
    /// Service-specific mutable draft type.
    type Draft: DraftResource;
    /// Service-specific metadata type.
    type Metadata;
    /// Service-specific upload specification type.
    type Upload;
    /// Service-specific draft file policy.
    type FilePolicy;
    /// Service-specific upload result type.
    type UploadResult;
    /// Result returned by the publish step.
    type Published;

    /// Creates a new mutable draft-like resource with the supplied metadata.
    fn create_draft(
        &self,
        metadata: &Self::Metadata,
    ) -> impl Future<Output = Result<Self::Draft, Self::Error>>;

    /// Replaces or merges the draft metadata.
    fn update_draft_metadata(
        &self,
        draft_id: &<Self::Draft as DraftResource>::Id,
        metadata: &Self::Metadata,
    ) -> impl Future<Output = Result<Self::Draft, Self::Error>>;

    /// Reconciles the draft file set under the requested policy.
    fn reconcile_draft_files(
        &self,
        draft: &Self::Draft,
        policy: Self::FilePolicy,
        uploads: Vec<Self::Upload>,
    ) -> impl Future<Output = Result<Vec<Self::UploadResult>, Self::Error>>;

    /// Publishes the draft-like resource.
    fn publish_draft(
        &self,
        draft_id: &<Self::Draft as DraftResource>::Id,
    ) -> impl Future<Output = Result<Self::Published, Self::Error>>;
}

/// Bundle for the capability set shared by all three current uploader clients.
pub trait CoreRepositoryClient:
    ClientContext
    + CreatePublication
    + DownloadNamedPublicFile
    + ListResourceFiles
    + ReadPublicResource
    + SearchPublicResources
    + UpdatePublication
{
}

impl<T> CoreRepositoryClient for T where
    T: ClientContext
        + CreatePublication
        + DownloadNamedPublicFile
        + ListResourceFiles
        + ReadPublicResource
        + SearchPublicResources
        + UpdatePublication
{
}

/// Bundle for clients that additionally support DOI lookup and latest-version resolution.
pub trait DoiVersionedRepositoryClient:
    CoreRepositoryClient + LookupByDoi + ResolveLatestPublicResource + ResolveLatestPublicResourceByDoi
{
}

impl<T> DoiVersionedRepositoryClient for T where
    T: CoreRepositoryClient
        + LookupByDoi
        + ResolveLatestPublicResource
        + ResolveLatestPublicResourceByDoi
{
}

/// Bundle for clients that expose draft-oriented mutation workflows.
pub trait DraftPublishingRepositoryClient: CoreRepositoryClient + DraftWorkflow {}

impl<T> DraftPublishingRepositoryClient for T where T: CoreRepositoryClient + DraftWorkflow {}

/// Returns the first file whose name matches `name`.
#[must_use]
pub fn find_file_by_name<'a, File>(
    files: impl IntoIterator<Item = &'a File>,
    name: &str,
) -> Option<&'a File>
where
    File: RepositoryFile + ?Sized + 'a,
{
    files.into_iter().find(|file| file.file_name() == name)
}

/// Returns whether any file in the iterator matches `name`.
#[must_use]
pub fn has_file_named<'a, File>(files: impl IntoIterator<Item = &'a File>, name: &str) -> bool
where
    File: RepositoryFile + ?Sized + 'a,
{
    find_file_by_name(files, name).is_some()
}

/// Returns the first embedded file on a record payload whose name matches `name`.
#[must_use]
pub fn find_embedded_record_file_by_name<'a, Record>(
    record: &'a Record,
    name: &str,
) -> Option<&'a Record::File>
where
    Record: RepositoryRecord + ?Sized,
{
    find_file_by_name(record.files().iter(), name)
}

/// Returns the first embedded file on a draft payload whose name matches `name`.
#[must_use]
pub fn find_embedded_draft_file_by_name<'a, Draft>(
    draft: &'a Draft,
    name: &str,
) -> Option<&'a Draft::File>
where
    Draft: DraftResource + ?Sized,
{
    find_file_by_name(draft.files().iter(), name)
}

/// Collects the set of upload filenames while validating that names are non-empty and unique.
///
/// The returned set can be reused for draft-reconciliation and conflict checks.
///
/// # Errors
///
/// Returns [`UploadNameValidationError::EmptyFilename`] when one upload has an
/// empty remote filename, or [`UploadNameValidationError::DuplicateFilename`]
/// when multiple uploads target the same remote filename.
pub fn collect_upload_filenames<'a, Upload>(
    uploads: impl IntoIterator<Item = &'a Upload>,
) -> Result<BTreeSet<String>, UploadNameValidationError>
where
    Upload: UploadSpecLike + ?Sized + 'a,
{
    let mut filenames = BTreeSet::new();
    for upload in uploads {
        let filename = upload.filename();
        if filename.is_empty() {
            return Err(UploadNameValidationError::EmptyFilename);
        }
        if !filenames.insert(filename.to_owned()) {
            return Err(UploadNameValidationError::DuplicateFilename {
                filename: filename.to_owned(),
            });
        }
    }
    Ok(filenames)
}

/// Validates that upload filenames are non-empty and unique.
///
/// # Errors
///
/// Returns the same validation errors as [`collect_upload_filenames`].
pub fn validate_upload_filenames<'a, Upload>(
    uploads: impl IntoIterator<Item = &'a Upload>,
) -> Result<(), UploadNameValidationError>
where
    Upload: UploadSpecLike + ?Sized + 'a,
{
    collect_upload_filenames(uploads).map(|_| ())
}

/// Returns whether any upload in the iterator targets `filename`.
#[must_use]
pub fn has_upload_filename<'a, Upload>(
    uploads: impl IntoIterator<Item = &'a Upload>,
    filename: &str,
) -> bool
where
    Upload: UploadSpecLike + ?Sized + 'a,
{
    uploads
        .into_iter()
        .any(|upload| upload.filename() == filename)
}

/// Common re-exports for crate consumers and implementers.
pub mod prelude {
    pub use crate::{
        collect_upload_filenames, find_embedded_draft_file_by_name,
        find_embedded_record_file_by_name, find_file_by_name, has_file_named, has_upload_filename,
        validate_upload_filenames, ClientContext, CoreRepositoryClient, CreatePublication,
        CreatePublicationRequest, DoiBackedRecord, DoiVersionedRepositoryClient,
        DownloadNamedPublicFile, DraftFilePolicy, DraftFilePolicyKind,
        DraftPublishingRepositoryClient, DraftResource, DraftState, DraftWorkflow,
        ExistingFileConflictPolicy, ExistingFileConflictPolicyKind, ListResourceFiles, LookupByDoi,
        MaybeAuthenticatedClient, MutablePublicationOutcome, NoCreateTarget, PublicationOutcome,
        ReadPublicResource, RepositoryFile, RepositoryRecord, ResolveLatestPublicResource,
        ResolveLatestPublicResourceByDoi, SearchPublicResources, SearchResultsLike,
        UpdatePublication, UpdatePublicationRequest, UploadNameValidationError, UploadSourceKind,
        UploadSpecLike,
    };
}

#[cfg(test)]
mod self_tests;
