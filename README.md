# client-uploader-traits

[![CI](https://github.com/LucaCappelletti94/client-uploader-traits/actions/workflows/ci.yml/badge.svg)](https://github.com/LucaCappelletti94/client-uploader-traits/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/client-uploader-traits.svg)](https://crates.io/crates/client-uploader-traits)
[![docs.rs](https://img.shields.io/docsrs/client-uploader-traits)](https://docs.rs/client-uploader-traits)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/LucaCappelletti94/client-uploader-traits/blob/main/LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.86%2B-orange.svg)](https://www.rust-lang.org)

Traits defining a common generic traits surface for uploader clients such as [`zenodo-rs`](https://github.com/LucaCappelletti94/zenodo-rs), [`internetarchive-rs`](https://github.com/LucaCappelletti94/internetarchive-rs), and [`figshare-rs`](https://github.com/LucaCappelletti94/figshare-rs).

It covers:

- client configuration
- upload, file, and resource inspection
- public lookup, search, and download capabilities
- create/update publication workflows
- publication outcome and search-result inspection
- small generic helpers and a `prelude`

```rust
use client_uploader_traits::{
    CreatePublication, CreatePublicationRequest, PublicationOutcome,
};

async fn publish<C>(
    client: &C,
    request: CreatePublicationRequest<C::CreateTarget, C::Metadata, C::Upload>,
) -> Result<C::Output, C::Error>
where
    C: CreatePublication,
    C::Output: PublicationOutcome,
{
    client.create_publication(request).await
}
```

This crate is intentionally self-contained. Real conformance tests against concrete client crates should live in those crates.
