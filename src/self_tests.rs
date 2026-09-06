use std::future::{ready, Future};
use std::path::Path;
use std::time::Duration;

use crate::{
    collect_upload_filenames, find_embedded_draft_file_by_name, find_embedded_record_file_by_name,
    find_file_by_name, has_file_named, has_upload_filename, validate_upload_filenames,
    ClientContext, CoreRepositoryClient, CreatePublication, CreatePublicationRequest,
    DoiBackedRecord, DoiVersionedRepositoryClient, DownloadNamedPublicFile, DraftFilePolicy,
    DraftFilePolicyKind, DraftPublishingRepositoryClient, DraftResource, DraftState, DraftWorkflow,
    ExistingFileConflictPolicy, ExistingFileConflictPolicyKind, ListResourceFiles, LookupByDoi,
    MaybeAuthenticatedClient, MutablePublicationOutcome, NoCreateTarget, PublicationOutcome,
    ReadPublicResource, RepositoryFile, RepositoryRecord, ResolveLatestPublicResource,
    ResolveLatestPublicResourceByDoi, SearchPublicResources, SearchResultsLike, UpdatePublication,
    UpdatePublicationRequest, UploadNameValidationError, UploadSourceKind, UploadSpecLike,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct DummyUpload {
    filename: &'static str,
    source_kind: UploadSourceKind,
}

impl UploadSpecLike for DummyUpload {
    fn filename(&self) -> &str {
        self.filename
    }

    fn source_kind(&self) -> UploadSourceKind {
        self.source_kind
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DummyFile {
    id: u64,
    name: &'static str,
    size: u64,
}

impl RepositoryFile for DummyFile {
    type Id = u64;

    fn file_id(&self) -> Option<Self::Id> {
        Some(self.id)
    }

    fn file_name(&self) -> &str {
        self.name
    }

    fn size_bytes(&self) -> Option<u64> {
        Some(self.size)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DummyRecord {
    id: u64,
    title: &'static str,
    doi: Option<&'static str>,
    files: Vec<DummyFile>,
}

impl RepositoryRecord for DummyRecord {
    type Id = u64;
    type File = DummyFile;

    fn resource_id(&self) -> Option<Self::Id> {
        Some(self.id)
    }

    fn title(&self) -> Option<&str> {
        Some(self.title)
    }

    fn files(&self) -> &[Self::File] {
        &self.files
    }
}

impl DoiBackedRecord for DummyRecord {
    type Doi = &'static str;

    fn doi(&self) -> Option<Self::Doi> {
        self.doi
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DummyDraft {
    id: u64,
    published: bool,
    editable: bool,
    files: Vec<DummyFile>,
}

impl DraftResource for DummyDraft {
    type Id = u64;
    type File = DummyFile;

    fn draft_id(&self) -> Self::Id {
        self.id
    }

    fn files(&self) -> &[Self::File] {
        &self.files
    }
}

impl DraftState for DummyDraft {
    fn is_published(&self) -> bool {
        self.published
    }

    fn allows_metadata_updates(&self) -> bool {
        self.editable
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DummyPublicationOutcome {
    public_resource: DummyRecord,
    mutable_resource: Option<DummyDraft>,
    created: Option<bool>,
}

impl PublicationOutcome for DummyPublicationOutcome {
    type PublicResource = DummyRecord;

    fn public_resource(&self) -> &Self::PublicResource {
        &self.public_resource
    }

    fn created(&self) -> Option<bool> {
        self.created
    }
}

impl MutablePublicationOutcome for DummyPublicationOutcome {
    type MutableResource = DummyDraft;

    fn mutable_resource(&self) -> Option<&Self::MutableResource> {
        self.mutable_resource.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DummySearchResults {
    items: Vec<DummyRecord>,
    total_hits: Option<u64>,
}

impl SearchResultsLike for DummySearchResults {
    type Item = DummyRecord;

    fn items(&self) -> &[Self::Item] {
        &self.items
    }

    fn total_hits(&self) -> Option<u64> {
        self.total_hits
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DummyDraftPolicy {
    ReplaceAll,
    UpsertByFilename,
    KeepExistingAndAdd,
}

impl DraftFilePolicy for DummyDraftPolicy {
    fn kind(&self) -> DraftFilePolicyKind {
        match self {
            Self::ReplaceAll => DraftFilePolicyKind::ReplaceAll,
            Self::UpsertByFilename => DraftFilePolicyKind::UpsertByFilename,
            Self::KeepExistingAndAdd => DraftFilePolicyKind::KeepExistingAndAdd,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DummyConflictPolicy {
    Error,
    Skip,
    Overwrite,
    OverwriteKeepingHistory,
}

impl ExistingFileConflictPolicy for DummyConflictPolicy {
    fn kind(&self) -> ExistingFileConflictPolicyKind {
        match self {
            Self::Error => ExistingFileConflictPolicyKind::Error,
            Self::Skip => ExistingFileConflictPolicyKind::Skip,
            Self::Overwrite => ExistingFileConflictPolicyKind::Overwrite,
            Self::OverwriteKeepingHistory => {
                ExistingFileConflictPolicyKind::OverwriteKeepingHistory
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DummyClient {
    authenticated: bool,
}

impl ClientContext for DummyClient {
    type Endpoint = &'static str;
    type PollOptions = Duration;
    type Error = std::convert::Infallible;

    fn endpoint(&self) -> &Self::Endpoint {
        static ENDPOINT: &str = "https://example.invalid/api";
        &ENDPOINT
    }

    fn poll_options(&self) -> &Self::PollOptions {
        static POLL_OPTIONS: Duration = Duration::from_secs(1);
        &POLL_OPTIONS
    }

    fn request_timeout(&self) -> Option<Duration> {
        Some(Duration::from_secs(30))
    }

    fn connect_timeout(&self) -> Option<Duration> {
        Some(Duration::from_secs(5))
    }
}

impl MaybeAuthenticatedClient for DummyClient {
    fn has_auth(&self) -> bool {
        self.authenticated
    }
}

impl ReadPublicResource for DummyClient {
    type ResourceId = u64;
    type Resource = DummyRecord;

    fn get_public_resource(
        &self,
        id: &Self::ResourceId,
    ) -> impl Future<Output = Result<Self::Resource, Self::Error>> {
        ready(Ok(dummy_record(*id)))
    }
}

impl SearchPublicResources for DummyClient {
    type Query = &'static str;
    type SearchResults = DummySearchResults;

    fn search_public_resources(
        &self,
        query: &Self::Query,
    ) -> impl Future<Output = Result<Self::SearchResults, Self::Error>> {
        ready(Ok(DummySearchResults {
            items: vec![dummy_record(query.len() as u64)],
            total_hits: Some(1),
        }))
    }
}

impl ListResourceFiles for DummyClient {
    type ResourceId = u64;
    type File = DummyFile;

    fn list_resource_files(
        &self,
        id: &Self::ResourceId,
    ) -> impl Future<Output = Result<Vec<Self::File>, Self::Error>> {
        ready(Ok(dummy_record(*id).files))
    }
}

impl DownloadNamedPublicFile for DummyClient {
    type ResourceId = u64;
    type Download = String;

    fn download_named_public_file_to_path(
        &self,
        id: &Self::ResourceId,
        name: &str,
        path: &Path,
    ) -> impl Future<Output = Result<Self::Download, Self::Error>> {
        ready(Ok(format!(
            "downloaded {name} from {id} to {}",
            path.display()
        )))
    }
}

impl CreatePublication for DummyClient {
    type CreateTarget = NoCreateTarget;
    type Metadata = &'static str;
    type Upload = DummyUpload;
    type Output = DummyPublicationOutcome;

    fn create_publication(
        &self,
        request: CreatePublicationRequest<Self::CreateTarget, Self::Metadata, Self::Upload>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> {
        ready(Ok(DummyPublicationOutcome {
            public_resource: DummyRecord {
                id: request.uploads.len() as u64,
                title: request.metadata,
                doi: Some("10.0000/dummy"),
                files: vec![DummyFile {
                    id: 1,
                    name: "artifact.bin",
                    size: 3,
                }],
            },
            mutable_resource: Some(dummy_draft(1, false, true)),
            created: Some(true),
        }))
    }
}

impl UpdatePublication for DummyClient {
    type ResourceId = u64;
    type Metadata = &'static str;
    type FilePolicy = DummyConflictPolicy;
    type Upload = DummyUpload;
    type Output = DummyPublicationOutcome;

    fn update_publication(
        &self,
        request: UpdatePublicationRequest<
            Self::ResourceId,
            Self::Metadata,
            Self::FilePolicy,
            Self::Upload,
        >,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> {
        let _ = request.policy.kind();
        ready(Ok(DummyPublicationOutcome {
            public_resource: DummyRecord {
                id: request.resource_id,
                title: request.metadata,
                doi: Some("10.0000/updated"),
                files: vec![DummyFile {
                    id: 2,
                    name: "manifest.json",
                    size: request.uploads.len() as u64,
                }],
            },
            mutable_resource: None,
            created: Some(false),
        }))
    }
}

impl LookupByDoi for DummyClient {
    type Doi = &'static str;
    type Resource = DummyRecord;

    fn get_public_resource_by_doi(
        &self,
        doi: &Self::Doi,
    ) -> impl Future<Output = Result<Self::Resource, Self::Error>> {
        ready(Ok(DummyRecord {
            id: doi.len() as u64,
            title: "by-doi",
            doi: Some(*doi),
            files: vec![],
        }))
    }
}

impl ResolveLatestPublicResource for DummyClient {
    type ResourceId = u64;
    type Resource = DummyRecord;

    fn resolve_latest_public_resource(
        &self,
        id: &Self::ResourceId,
    ) -> impl Future<Output = Result<Self::Resource, Self::Error>> {
        ready(Ok(dummy_record(*id + 1)))
    }
}

impl ResolveLatestPublicResourceByDoi for DummyClient {
    type Doi = &'static str;
    type Resource = DummyRecord;

    fn resolve_latest_public_resource_by_doi(
        &self,
        doi: &Self::Doi,
    ) -> impl Future<Output = Result<Self::Resource, Self::Error>> {
        ready(Ok(DummyRecord {
            id: doi.len() as u64 + 1,
            title: "latest-by-doi",
            doi: Some(*doi),
            files: vec![],
        }))
    }
}

impl DraftWorkflow for DummyClient {
    type Draft = DummyDraft;
    type Metadata = &'static str;
    type Upload = DummyUpload;
    type FilePolicy = DummyDraftPolicy;
    type UploadResult = DummyFile;
    type Published = DummyDraft;

    fn create_draft(
        &self,
        _metadata: &Self::Metadata,
    ) -> impl Future<Output = Result<Self::Draft, Self::Error>> {
        ready(Ok(dummy_draft(1, false, true)))
    }

    fn update_draft_metadata(
        &self,
        draft_id: &<Self::Draft as DraftResource>::Id,
        _metadata: &Self::Metadata,
    ) -> impl Future<Output = Result<Self::Draft, Self::Error>> {
        ready(Ok(dummy_draft(*draft_id, false, true)))
    }

    fn reconcile_draft_files(
        &self,
        draft: &Self::Draft,
        policy: Self::FilePolicy,
        uploads: Vec<Self::Upload>,
    ) -> impl Future<Output = Result<Vec<Self::UploadResult>, Self::Error>> {
        let _ = draft.id;
        let _ = policy.kind();
        ready(Ok(uploads
            .into_iter()
            .enumerate()
            .map(|(index, upload)| DummyFile {
                id: index as u64,
                name: if upload.filename.is_empty() {
                    "unnamed"
                } else {
                    upload.filename
                },
                size: 1,
            })
            .collect()))
    }

    fn publish_draft(
        &self,
        draft_id: &<Self::Draft as DraftResource>::Id,
    ) -> impl Future<Output = Result<Self::Published, Self::Error>> {
        ready(Ok(dummy_draft(*draft_id, true, false)))
    }
}

fn dummy_record(id: u64) -> DummyRecord {
    DummyRecord {
        id,
        title: "dummy",
        doi: Some("10.0000/dummy"),
        files: vec![
            DummyFile {
                id: 1,
                name: "artifact.bin",
                size: 3,
            },
            DummyFile {
                id: 2,
                name: "manifest.json",
                size: 7,
            },
        ],
    }
}

fn dummy_draft(id: u64, published: bool, editable: bool) -> DummyDraft {
    DummyDraft {
        id,
        published,
        editable,
        files: vec![
            DummyFile {
                id: 10,
                name: "artifact.bin",
                size: 3,
            },
            DummyFile {
                id: 11,
                name: "manifest.json",
                size: 7,
            },
        ],
    }
}

fn assert_core_client<C>()
where
    C: CoreRepositoryClient,
{
}

fn assert_doi_client<C>()
where
    C: DoiVersionedRepositoryClient,
{
}

fn assert_draft_client<C>()
where
    C: DraftPublishingRepositoryClient,
{
}

fn assert_publication_outcome<O>()
where
    O: PublicationOutcome,
{
}

fn assert_mutable_publication_outcome<O>()
where
    O: MutablePublicationOutcome,
{
}

fn assert_search_results_like<S>()
where
    S: SearchResultsLike,
{
}

#[test]
fn bundle_traits_compile_for_dummy_client() {
    assert_core_client::<DummyClient>();
    assert_doi_client::<DummyClient>();
    assert_draft_client::<DummyClient>();
}

#[test]
fn outcome_and_search_result_traits_compile_for_dummy_types() {
    assert_publication_outcome::<DummyPublicationOutcome>();
    assert_mutable_publication_outcome::<DummyPublicationOutcome>();
    assert_search_results_like::<DummySearchResults>();
    assert_search_results_like::<Vec<DummyRecord>>();
}

#[test]
fn auth_and_policy_traits_work() {
    let client = DummyClient {
        authenticated: true,
    };

    assert!(client.has_auth());
    assert_eq!(
        DummyDraftPolicy::ReplaceAll.kind(),
        DraftFilePolicyKind::ReplaceAll
    );
    assert_eq!(
        DummyDraftPolicy::UpsertByFilename.kind(),
        DraftFilePolicyKind::UpsertByFilename
    );
    assert_eq!(
        DummyDraftPolicy::KeepExistingAndAdd.kind(),
        DraftFilePolicyKind::KeepExistingAndAdd
    );
    assert_eq!(
        DummyConflictPolicy::Error.kind(),
        ExistingFileConflictPolicyKind::Error
    );
    assert_eq!(
        DummyConflictPolicy::Skip.kind(),
        ExistingFileConflictPolicyKind::Skip
    );
    assert_eq!(
        DummyConflictPolicy::Overwrite.kind(),
        ExistingFileConflictPolicyKind::Overwrite
    );
    assert_eq!(
        DummyConflictPolicy::OverwriteKeepingHistory.kind(),
        ExistingFileConflictPolicyKind::OverwriteKeepingHistory
    );
}

#[test]
fn request_helpers_preserve_payloads() {
    let create = CreatePublicationRequest::untargeted("meta", vec![1]).with_upload(2);
    assert_eq!(create.target, NoCreateTarget);
    assert_eq!(create.metadata, "meta");
    assert_eq!(create.uploads, vec![1, 2]);

    let remapped = create.map_uploads(|value| value.to_string());
    assert_eq!(remapped.uploads, vec!["1".to_owned(), "2".to_owned()]);

    let update = UpdatePublicationRequest::new(7_u64, "meta", "policy", vec![1]).with_upload(2);
    assert_eq!(update.resource_id, 7);
    assert_eq!(update.metadata, "meta");
    assert_eq!(update.policy, "policy");
    assert_eq!(update.uploads, vec![1, 2]);
}

#[test]
fn upload_filename_helpers_validate_and_collect() -> Result<(), Box<dyn std::error::Error>> {
    let uploads = [
        DummyUpload {
            filename: "artifact.bin",
            source_kind: UploadSourceKind::Path,
        },
        DummyUpload {
            filename: "manifest.json",
            source_kind: UploadSourceKind::Bytes,
        },
    ];

    let filenames = collect_upload_filenames(uploads.iter())?;
    assert!(filenames.contains("artifact.bin"));
    assert!(filenames.contains("manifest.json"));
    assert!(has_upload_filename(uploads.iter(), "artifact.bin"));
    assert!(!has_upload_filename(uploads.iter(), "missing.txt"));
    assert!(validate_upload_filenames(uploads.iter()).is_ok());
    Ok(())
}

#[test]
fn upload_filename_helpers_reject_empty_and_duplicate_names() {
    let empty = [DummyUpload {
        filename: "",
        source_kind: UploadSourceKind::Reader,
    }];
    assert_eq!(
        validate_upload_filenames(empty.iter()),
        Err(UploadNameValidationError::EmptyFilename)
    );

    let duplicate = [
        DummyUpload {
            filename: "artifact.bin",
            source_kind: UploadSourceKind::Path,
        },
        DummyUpload {
            filename: "artifact.bin",
            source_kind: UploadSourceKind::Bytes,
        },
    ];
    assert_eq!(
        collect_upload_filenames(duplicate.iter()),
        Err(UploadNameValidationError::DuplicateFilename {
            filename: "artifact.bin".to_owned(),
        })
    );
}

#[test]
fn file_lookup_helpers_work_for_generic_record_and_draft_types(
) -> Result<(), Box<dyn std::error::Error>> {
    let record = dummy_record(42);
    let draft = dummy_draft(24, false, true);

    let file = find_file_by_name(record.files.iter(), "manifest.json")
        .ok_or_else(|| std::io::Error::other("missing file"))?;
    assert_eq!(file.file_id(), Some(2));
    assert!(has_file_named(record.files.iter(), "artifact.bin"));
    assert!(find_embedded_record_file_by_name(&record, "artifact.bin").is_some());
    assert!(find_embedded_draft_file_by_name(&draft, "manifest.json").is_some());
    assert!(find_embedded_record_file_by_name(&record, "missing.txt").is_none());
    Ok(())
}

#[test]
fn search_result_helpers_report_counts() {
    let results = DummySearchResults {
        items: vec![dummy_record(1), dummy_record(2)],
        total_hits: Some(10),
    };

    assert_eq!(results.page_len(), 2);
    assert!(!results.is_empty());
    assert_eq!(results.total_hits(), Some(10));
}

#[test]
fn outcome_traits_expose_standardized_result_access() {
    let outcome = DummyPublicationOutcome {
        public_resource: dummy_record(5),
        mutable_resource: Some(dummy_draft(5, false, true)),
        created: Some(true),
    };

    assert_eq!(outcome.public_resource().resource_id(), Some(5));
    assert_eq!(outcome.created(), Some(true));
    assert_eq!(
        outcome.mutable_resource().map(DraftResource::draft_id),
        Some(5)
    );
}
