feat(contracts/pacs): implement PACS/DICOM imaging Soroban smart contract

Add `contracts/pacs` — a Soroban smart contract covering full Picture
Archiving and Communication System (PACS) functionality anchored on the
Stellar blockchain.

## Functions implemented

| Function | Description |
|---|---|
| `register_imaging_study` | Register DICOM study with UID, modality, body part, series/image counts, storage hash |
| `add_series_to_study` | Append a DICOM series to an existing study |
| `link_imaging_report` | Attach a radiology report hash; emits event on critical findings |
| `request_comparison_study` | Return prior study IDs matching modality, body part, recency |
| `grant_imaging_access` | Patient-controlled ACL with optional expiry |
| `create_imaging_cd` | Bundle studies into a portable CD record |
| `anonymize_study` | Admin-only de-identification with on-chain audit event |
| `quality_control_review` | Record QC score and technical issues |
| `track_study_views` | Append-only audit log enforcing access grants |
| `search_imaging_studies` | Filtered search by modality, body part, date range, critical findings |

## Files

- `contracts/Cargo.toml` — workspace manifest
- `contracts/pacs/Cargo.toml` — crate manifest (`soroban-sdk = 21`)
- `contracts/pacs/src/lib.rs` — contract + all data structures + storage keys
- `contracts/pacs/src/tests.rs` — 19 unit tests (all passing)

## Test results

```
running 19 tests
test result: ok. 19 passed; 0 failed
```

closes: #56
