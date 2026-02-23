use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, String, Symbol, Vec,
};

use crate::{ComparisonCriteria, ImagingFilters, PacsContract, PacsContractClient};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn setup() -> (Env, PacsContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, PacsContract);
    let client = PacsContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

fn hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xABu8; 32])
}

fn register(
    env: &Env,
    client: &PacsContractClient,
    patient: &Address,
    provider: &Address,
    uid: &str,
    modality: &str,
    body: &str,
    date: u64,
) -> u64 {
    client.register_imaging_study(
        patient,
        provider,
        &String::from_str(env, uid),
        &Symbol::new(env, modality),
        &String::from_str(env, body),
        &date,
        &String::from_str(env, "desc"),
        &2,
        &60,
        &hash(env),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

/// A study can be registered and retrieved with the correct metadata.
#[test]
fn test_register_and_get_study() {
    let (env, client, _) = setup();
    let patient = Address::generate(&env);
    let provider = Address::generate(&env);

    let sid = register(&env, &client, &patient, &provider, "1.2.3", "CT", "CHEST", 1_700_000_000);

    let study = client.get_study(&sid);
    assert_eq!(study.study_id, 1);
    assert_eq!(study.modality, Symbol::new(&env, "CT"));
    assert!(!study.has_report);
    assert!(!study.critical_findings);
    assert!(!study.is_anonymized);
}

/// Auto-increment produces sequential IDs.
#[test]
fn test_study_id_increments() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);

    let s1 = register(&env, &client, &p, &prov, "1.1", "CT", "CHEST", 0);
    let s2 = register(&env, &client, &p, &prov, "1.2", "MRI", "BRAIN", 0);
    assert_eq!(s2, s1 + 1);
}

/// A series can be appended to a study; series list grows.
#[test]
fn test_add_series() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let sid = register(&env, &client, &p, &prov, "2.1", "MRI", "BRAIN", 0);

    client.add_series_to_study(
        &sid,
        &String::from_str(&env, "2.1.1"),
        &1,
        &String::from_str(&env, "T1"),
        &30,
        &100,
    );

    let series = client.get_study_series(&sid);
    assert_eq!(series.len(), 1);
    assert_eq!(series.get_unchecked(0).series_number, 1);
}

/// Multiple series accumulate in order.
#[test]
fn test_add_multiple_series() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let sid = register(&env, &client, &p, &prov, "3.1", "CT", "CHEST", 0);

    for i in 1u32..=3 {
        client.add_series_to_study(
            &sid,
            &String::from_str(&env, "uid"),
            &i,
            &String::from_str(&env, "desc"),
            &10,
            &0,
        );
    }

    assert_eq!(client.get_study_series(&sid).len(), 3);
}

/// Linking a report sets has_report and critical_findings flags.
#[test]
fn test_link_report_critical() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let rad = Address::generate(&env);
    let sid = register(&env, &client, &p, &prov, "4.1", "XR", "CHEST", 0);

    client.link_imaging_report(
        &sid,
        &rad,
        &Symbol::new(&env, "final"),
        &hash(&env),
        &true,
    );

    let study = client.get_study(&sid);
    assert!(study.has_report);
    assert!(study.critical_findings);

    let report = client.get_report(&sid);
    assert!(report.critical_findings);
}

/// Linking a second report on the same study returns an error.
#[test]
fn test_duplicate_report_rejected() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let rad = Address::generate(&env);
    let sid = register(&env, &client, &p, &prov, "5.1", "US", "ABDOMEN", 0);

    client.link_imaging_report(&sid, &rad, &Symbol::new(&env, "preliminary"), &hash(&env), &false);

    assert!(client
        .try_link_imaging_report(&sid, &rad, &Symbol::new(&env, "final"), &hash(&env), &false)
        .is_err());
}

/// Patient can grant access; viewer can then track a study view.
#[test]
fn test_grant_access_and_track_view() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let viewer = Address::generate(&env);
    let sid = register(&env, &client, &p, &prov, "6.1", "CT", "CHEST", 0);

    client.grant_imaging_access(&sid, &p, &viewer, &Symbol::new(&env, "view_only"), &None);
    client.track_study_views(&sid, &viewer, &500, &30);

    let views = client.get_study_views(&sid);
    assert_eq!(views.len(), 1);
    assert_eq!(views.get_unchecked(0).view_duration, 30);
}

/// Ordering provider has implicit access without an explicit grant.
#[test]
fn test_ordering_provider_implicit_access() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let sid = register(&env, &client, &p, &prov, "7.1", "MRI", "SPINE", 0);

    client.track_study_views(&sid, &prov, &1_000, &60);
    assert_eq!(client.get_study_views(&sid).len(), 1);
}

/// A viewer without a grant cannot track views.
#[test]
fn test_no_grant_rejected() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let stranger = Address::generate(&env);
    let sid = register(&env, &client, &p, &prov, "8.1", "CT", "KNEE", 0);

    assert!(client
        .try_track_study_views(&sid, &stranger, &1_000, &5)
        .is_err());
}

/// An expired access grant is refused.
#[test]
fn test_access_expired() {
    let (env, client, _) = setup();
    env.ledger().set_timestamp(1_000);
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let viewer = Address::generate(&env);
    let sid = register(&env, &client, &p, &prov, "9.1", "MRI", "KNEE", 1_000);

    // Grant that has already expired
    client.grant_imaging_access(&sid, &p, &viewer, &Symbol::new(&env, "view_only"), &Some(500u64));

    env.ledger().set_timestamp(2_000);

    assert!(client
        .try_track_study_views(&sid, &viewer, &2_000, &10)
        .is_err());
}

/// `request_comparison_study` returns prior studies matching modality + body part.
#[test]
fn test_comparison_study() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let rad = Address::generate(&env);

    let s1 = register(&env, &client, &p, &prov, "10.1", "CT",  "CHEST", 1_600_000_000);
    let s2 = register(&env, &client, &p, &prov, "10.2", "CT",  "CHEST", 1_680_000_000);
    let s3 = register(&env, &client, &p, &prov, "10.3", "MRI", "CHEST", 1_690_000_000);

    for &sid in &[s1, s2, s3] {
        client.grant_imaging_access(&sid, &p, &rad, &Symbol::new(&env, "view_only"), &None);
    }

    let criteria = ComparisonCriteria {
        modality: Some(Symbol::new(&env, "CT")),
        body_part: String::from_str(&env, "CHEST"),
        max_age_days: 3650,
        same_side: false,
    };

    // s2 is current; should find s1 (same modality+body) but not s3 (MRI)
    let matches = client.request_comparison_study(&s2, &rad, &criteria);
    assert!(matches.contains(&s1));
    assert!(!matches.contains(&s3));
}

/// `search_imaging_studies` respects the modality filter.
#[test]
fn test_search_modality_filter() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);

    register(&env, &client, &p, &prov, "20.1", "CT",  "HEAD",  1_700_000_000);
    register(&env, &client, &p, &prov, "20.2", "XR",  "CHEST", 1_700_100_000);
    register(&env, &client, &p, &prov, "20.3", "CT",  "CHEST", 1_700_200_000);

    let filters = ImagingFilters {
        modality: Some(Symbol::new(&env, "CT")),
        body_part: None,
        start_date: None,
        end_date: None,
        has_critical_findings: None,
    };

    let results = client.search_imaging_studies(&p, &p, &filters);
    assert_eq!(results.len(), 2);
    for i in 0..results.len() {
        assert_eq!(results.get_unchecked(i).modality, Symbol::new(&env, "CT"));
    }
}

/// `search_imaging_studies` can filter on `critical_findings`.
#[test]
fn test_search_critical_findings_filter() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let rad = Address::generate(&env);

    let s1 = register(&env, &client, &p, &prov, "21.1", "CT", "CHEST", 0);
    let s2 = register(&env, &client, &p, &prov, "21.2", "CT", "CHEST", 0);

    client.link_imaging_report(&s1, &rad, &Symbol::new(&env, "final"), &hash(&env), &true);
    client.link_imaging_report(&s2, &rad, &Symbol::new(&env, "final"), &hash(&env), &false);

    let filters = ImagingFilters {
        modality: None,
        body_part: None,
        start_date: None,
        end_date: None,
        has_critical_findings: Some(true),
    };

    let results = client.search_imaging_studies(&p, &p, &filters);
    assert_eq!(results.len(), 1);
    assert_eq!(results.get_unchecked(0).study_id, s1);
}

/// `create_imaging_cd` returns a CD record ID; rejects cross-patient bundles.
#[test]
fn test_create_imaging_cd() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let other = Address::generate(&env);

    let s1 = register(&env, &client, &p, &prov, "30.1", "CT",  "CHEST", 0);
    let s2 = register(&env, &client, &p, &prov, "30.2", "MRI", "BRAIN", 0);
    let s3 = register(&env, &client, &other, &prov, "30.3", "XR", "HAND", 0);

    let mut ids = Vec::new(&env);
    ids.push_back(s1);
    ids.push_back(s2);

    let cd_id = client.create_imaging_cd(
        &ids,
        &p,
        &prov,
        &String::from_str(&env, "TOKEN-XYZ"),
        &999,
    );
    assert_eq!(cd_id, 1);

    // Mixing patients must fail
    let mut bad_ids = Vec::new(&env);
    bad_ids.push_back(s1);
    bad_ids.push_back(s3);

    assert!(client
        .try_create_imaging_cd(&bad_ids, &p, &prov, &String::from_str(&env, "BAD"), &0)
        .is_err());
}

/// `anonymize_study` requires admin privileges.
#[test]
fn test_anonymize_study_admin_only() {
    let (env, client, admin) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let rando = Address::generate(&env);

    let sid = register(&env, &client, &p, &prov, "40.1", "CT", "CHEST", 0);

    // Non-admin fails
    assert!(client
        .try_anonymize_study(
            &sid,
            &rando,
            &Symbol::new(&env, "full"),
            &String::from_str(&env, "research"),
        )
        .is_err());

    // Admin succeeds
    let anon_uid = client.anonymize_study(
        &sid,
        &admin,
        &Symbol::new(&env, "full"),
        &String::from_str(&env, "oncology research"),
    );
    assert_eq!(anon_uid, String::from_str(&env, "ANONYMIZED"));

    assert!(client.get_study(&sid).is_anonymized);
}

/// `quality_control_review` stores the review; rejects scores > 100.
#[test]
fn test_quality_control_review() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let reviewer = Address::generate(&env);

    let sid = register(&env, &client, &p, &prov, "50.1", "US", "PELVIS", 0);
    let issues: Vec<String> = Vec::new(&env);

    client.quality_control_review(&sid, &reviewer, &85, &issues, &false);

    let review = client.get_qc_review(&sid);
    assert_eq!(review.quality_score, 85);
    assert!(!review.repeat_required);
}

#[test]
fn test_quality_control_score_out_of_range() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let reviewer = Address::generate(&env);

    let sid = register(&env, &client, &p, &prov, "51.1", "MRI", "SPINE", 0);
    let issues: Vec<String> = Vec::new(&env);

    assert!(client
        .try_quality_control_review(&sid, &reviewer, &101, &issues, &false)
        .is_err());
}

/// Multiple views from different viewers are all appended to the audit log.
#[test]
fn test_multiple_views_audit_trail() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);
    let v1 = Address::generate(&env);
    let v2 = Address::generate(&env);

    let sid = register(&env, &client, &p, &prov, "60.1", "CT", "HEAD", 0);

    client.grant_imaging_access(&sid, &p, &v1, &Symbol::new(&env, "view_only"), &None);
    client.grant_imaging_access(&sid, &p, &v2, &Symbol::new(&env, "view_only"), &None);

    client.track_study_views(&sid, &v1, &1_000, &20);
    client.track_study_views(&sid, &v2, &2_000, &45);
    client.track_study_views(&sid, &v1, &3_000, &10);

    assert_eq!(client.get_study_views(&sid).len(), 3);
}

/// `register_imaging_study` rejects zero `image_count`.
#[test]
fn test_register_zero_images_rejected() {
    let (env, client, _) = setup();
    let p = Address::generate(&env);
    let prov = Address::generate(&env);

    assert!(client
        .try_register_imaging_study(
            &p,
            &prov,
            &String::from_str(&env, "bad"),
            &Symbol::new(&env, "CT"),
            &String::from_str(&env, "CHEST"),
            &0,
            &String::from_str(&env, "desc"),
            &1,
            &0,
            &hash(&env),
        )
        .is_err());
}
