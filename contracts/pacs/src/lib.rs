#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracterror, contracttype,
    Address, BytesN, Env, String, Symbol, Vec,
};

// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    StudyNotFound      = 1,
    Unauthorized       = 2,
    AccessExpired      = 3,
    AlreadyExists      = 4,
    InvalidInput       = 5,
    ReportAlreadyLinked = 6,
}

// ─────────────────────────────────────────────────────────────────────────────
// Domain types
// ─────────────────────────────────────────────────────────────────────────────

/// A DICOM imaging study anchored on-chain.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ImagingStudy {
    pub study_id:              u64,
    pub patient_id:            Address,
    pub ordering_provider:     Address,
    pub study_uid:             String,
    pub modality:              Symbol,
    pub body_part:             String,
    pub study_date:            u64,
    pub study_description:     String,
    pub series_count:          u32,
    pub image_count:           u32,
    pub storage_location_hash: BytesN<32>,
    pub has_report:            bool,
    pub critical_findings:     bool,
    pub is_anonymized:         bool,
    pub created_at:            u64,
}

/// One DICOM series belonging to a study.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ImagingSeries {
    pub study_id:           u64,
    pub series_uid:         String,
    pub series_number:      u32,
    pub series_description: String,
    pub image_count:        u32,
    pub acquisition_date:   u64,
}

/// Radiology report linked to a study.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ImagingReport {
    pub study_id:          u64,
    pub radiologist_id:    Address,
    pub report_type:       Symbol,
    pub report_hash:       BytesN<32>,
    pub critical_findings: bool,
    pub created_at:        u64,
}

/// Access grant given by a patient to a viewer.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AccessGrant {
    pub study_id:    u64,
    pub viewer_id:   Address,
    pub access_type: Symbol,
    pub granted_at:  u64,
    pub expires_at:  Option<u64>,
}

/// A portable CD record bundling several studies.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ImagingCd {
    pub cd_id:               u64,
    pub study_ids:           Vec<u64>,
    pub patient_id:          Address,
    pub requesting_provider: Address,
    pub cd_token:            String,
    pub created_at:          u64,
}

/// Immutable view-audit entry.
#[contracttype]
#[derive(Clone, Debug)]
pub struct StudyView {
    pub study_id:       u64,
    pub viewer_id:      Address,
    pub view_timestamp: u64,
    pub view_duration:  u32,
}

/// Quality-control review for a study.
#[contracttype]
#[derive(Clone, Debug)]
pub struct QcReview {
    pub study_id:         u64,
    pub reviewer_id:      Address,
    pub quality_score:    u32,
    pub technical_issues: Vec<String>,
    pub repeat_required:  bool,
    pub reviewed_at:      u64,
}

/// Criteria used when searching for prior comparison studies.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ComparisonCriteria {
    pub modality:     Option<Symbol>,
    pub body_part:    String,
    pub max_age_days: u32,
    pub same_side:    bool,
}

/// Filter set for `search_imaging_studies`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ImagingFilters {
    pub modality:             Option<Symbol>,
    pub body_part:            Option<String>,
    pub start_date:           Option<u64>,
    pub end_date:             Option<u64>,
    pub has_critical_findings: Option<bool>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Storage keys
// ─────────────────────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Admin,
    StudyCounter,
    CdCounter,
    Study(u64),
    Series(u64),
    Report(u64),
    AccessGrant(u64, Address),
    StudyViews(u64),
    QcReview(u64),
    CdRecord(u64),
    PatientStudies(Address),
}

// ─────────────────────────────────────────────────────────────────────────────
// Contract
// ─────────────────────────────────────────────────────────────────────────────

#[contract]
pub struct PacsContract;

#[contractimpl]
impl PacsContract {
    // ── Lifecycle ─────────────────────────────────────────────────────────────

    /// Initialise the contract; must be called once after deployment.
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::StudyCounter, &0u64);
        env.storage().instance().set(&DataKey::CdCounter, &0u64);
    }

    // ── Study Registration ────────────────────────────────────────────────────

    /// Register a new DICOM imaging study and return its on-chain study ID.
    pub fn register_imaging_study(
        env: Env,
        patient_id: Address,
        ordering_provider: Address,
        study_uid: String,
        modality: Symbol,
        body_part: String,
        study_date: u64,
        study_description: String,
        series_count: u32,
        image_count: u32,
        storage_location_hash: BytesN<32>,
    ) -> Result<u64, Error> {
        ordering_provider.require_auth();

        if image_count == 0 {
            return Err(Error::InvalidInput);
        }

        let study_id = Self::next_study_id(&env);

        let study = ImagingStudy {
            study_id,
            patient_id: patient_id.clone(),
            ordering_provider,
            study_uid,
            modality,
            body_part,
            study_date,
            study_description,
            series_count: series_count.max(1),
            image_count,
            storage_location_hash,
            has_report: false,
            critical_findings: false,
            is_anonymized: false,
            created_at: env.ledger().timestamp(),
        };

        env.storage().instance().set(&DataKey::Study(study_id), &study);
        Self::push_patient_study(&env, &patient_id, study_id);

        env.events().publish(
            (Symbol::new(&env, "study_registered"),),
            (study_id, patient_id),
        );

        Ok(study_id)
    }

    // ── Series Management ─────────────────────────────────────────────────────

    /// Append a DICOM series to an existing study.
    pub fn add_series_to_study(
        env: Env,
        study_id: u64,
        series_uid: String,
        series_number: u32,
        series_description: String,
        image_count: u32,
        acquisition_date: u64,
    ) -> Result<(), Error> {
        let mut study: ImagingStudy = env
            .storage()
            .instance()
            .get(&DataKey::Study(study_id))
            .ok_or(Error::StudyNotFound)?;

        study.ordering_provider.require_auth();

        let series = ImagingSeries {
            study_id,
            series_uid,
            series_number,
            series_description,
            image_count,
            acquisition_date,
        };

        let mut list: Vec<ImagingSeries> = env
            .storage()
            .instance()
            .get(&DataKey::Series(study_id))
            .unwrap_or_else(|| Vec::new(&env));

        list.push_back(series);
        study.series_count = list.len();

        env.storage().instance().set(&DataKey::Series(study_id), &list);
        env.storage().instance().set(&DataKey::Study(study_id), &study);

        Ok(())
    }

    // ── Report Linking ────────────────────────────────────────────────────────

    /// Link a radiology report hash to a study.
    ///
    /// Each study may only have one report (re-reads should use `addendum` via
    /// a separate call that overwrites); duplicate calls return an error until
    /// we explicitly support addenda.
    pub fn link_imaging_report(
        env: Env,
        study_id: u64,
        radiologist_id: Address,
        report_type: Symbol,
        report_hash: BytesN<32>,
        critical_findings: bool,
    ) -> Result<(), Error> {
        radiologist_id.require_auth();

        let mut study: ImagingStudy = env
            .storage()
            .instance()
            .get(&DataKey::Study(study_id))
            .ok_or(Error::StudyNotFound)?;

        if study.has_report {
            return Err(Error::ReportAlreadyLinked);
        }

        let report = ImagingReport {
            study_id,
            radiologist_id: radiologist_id.clone(),
            report_type,
            report_hash,
            critical_findings,
            created_at: env.ledger().timestamp(),
        };

        study.has_report        = true;
        study.critical_findings = critical_findings;

        env.storage().instance().set(&DataKey::Report(study_id), &report);
        env.storage().instance().set(&DataKey::Study(study_id), &study);

        if critical_findings {
            env.events().publish(
                (Symbol::new(&env, "critical_finding"),),
                (study_id, radiologist_id),
            );
        }

        Ok(())
    }

    // ── Comparison Study ──────────────────────────────────────────────────────

    /// Return prior study IDs for the same patient that satisfy
    /// `comparison_criteria` (modality, body part, recency window).
    pub fn request_comparison_study(
        env: Env,
        current_study_id: u64,
        radiologist_id: Address,
        comparison_criteria: ComparisonCriteria,
    ) -> Result<Vec<u64>, Error> {
        radiologist_id.require_auth();

        let current: ImagingStudy = env
            .storage()
            .instance()
            .get(&DataKey::Study(current_study_id))
            .ok_or(Error::StudyNotFound)?;

        Self::check_access(&env, current_study_id, &radiologist_id)?;

        let patient_studies: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::PatientStudies(current.patient_id.clone()))
            .unwrap_or_else(|| Vec::new(&env));

        let window_secs =
            (comparison_criteria.max_age_days as u64).saturating_mul(86_400);
        let cutoff = current.study_date.saturating_sub(window_secs);

        let mut matches: Vec<u64> = Vec::new(&env);

        for i in 0..patient_studies.len() {
            let sid = patient_studies.get_unchecked(i);
            if sid == current_study_id {
                continue;
            }
            if let Some(study) = env
                .storage()
                .instance()
                .get::<DataKey, ImagingStudy>(&DataKey::Study(sid))
            {
                let modality_ok = comparison_criteria
                    .modality
                    .as_ref()
                    .map(|m| m == &study.modality)
                    .unwrap_or(true);
                let body_ok = study.body_part == comparison_criteria.body_part;
                let date_ok = study.study_date >= cutoff;

                if modality_ok && body_ok && date_ok {
                    matches.push_back(sid);
                }
            }
        }

        Ok(matches)
    }

    // ── Access Control ────────────────────────────────────────────────────────

    /// Grant a viewer access to a study (patient must authorise).
    pub fn grant_imaging_access(
        env: Env,
        study_id: u64,
        patient_id: Address,
        viewer_id: Address,
        access_type: Symbol,
        expires_at: Option<u64>,
    ) -> Result<(), Error> {
        patient_id.require_auth();

        let study: ImagingStudy = env
            .storage()
            .instance()
            .get(&DataKey::Study(study_id))
            .ok_or(Error::StudyNotFound)?;

        if study.patient_id != patient_id {
            return Err(Error::Unauthorized);
        }

        let grant = AccessGrant {
            study_id,
            viewer_id: viewer_id.clone(),
            access_type,
            granted_at: env.ledger().timestamp(),
            expires_at,
        };

        env.storage()
            .instance()
            .set(&DataKey::AccessGrant(study_id, viewer_id.clone()), &grant);

        env.events().publish(
            (Symbol::new(&env, "access_granted"),),
            (study_id, viewer_id),
        );

        Ok(())
    }

    // ── CD Creation ───────────────────────────────────────────────────────────

    /// Bundle one or more studies into a portable CD record.
    pub fn create_imaging_cd(
        env: Env,
        study_ids: Vec<u64>,
        patient_id: Address,
        requesting_provider: Address,
        cd_token: String,
        created_at: u64,
    ) -> Result<u64, Error> {
        requesting_provider.require_auth();

        if study_ids.is_empty() {
            return Err(Error::InvalidInput);
        }

        for i in 0..study_ids.len() {
            let sid = study_ids.get_unchecked(i);
            let study: ImagingStudy = env
                .storage()
                .instance()
                .get(&DataKey::Study(sid))
                .ok_or(Error::StudyNotFound)?;
            if study.patient_id != patient_id {
                return Err(Error::Unauthorized);
            }
        }

        let cd_id = Self::next_cd_id(&env);
        let cd = ImagingCd {
            cd_id,
            study_ids,
            patient_id,
            requesting_provider,
            cd_token,
            created_at,
        };
        env.storage().instance().set(&DataKey::CdRecord(cd_id), &cd);

        Ok(cd_id)
    }

    // ── Anonymization ─────────────────────────────────────────────────────────

    /// Admin-controlled de-identification. Marks the study as anonymised
    /// and emits an audit event. Returns an anonymised study UID string.
    pub fn anonymize_study(
        env: Env,
        study_id: u64,
        requesting_researcher: Address,
        anonymization_level: Symbol,
        purpose: String,
    ) -> Result<String, Error> {
        requesting_researcher.require_auth();

        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::Unauthorized)?;

        if requesting_researcher != admin {
            return Err(Error::Unauthorized);
        }

        let mut study: ImagingStudy = env
            .storage()
            .instance()
            .get(&DataKey::Study(study_id))
            .ok_or(Error::StudyNotFound)?;

        study.is_anonymized = true;
        env.storage().instance().set(&DataKey::Study(study_id), &study);

        env.events().publish(
            (Symbol::new(&env, "study_anonymized"),),
            (study_id, requesting_researcher, anonymization_level, purpose),
        );

        // Return a stable anonymised UID token (de-identification happens
        // off-chain; the on-chain token simply marks the event).
        Ok(String::from_str(&env, "ANONYMIZED"))
    }

    // ── Quality Control ───────────────────────────────────────────────────────

    /// Record a quality-control review for a study.
    pub fn quality_control_review(
        env: Env,
        study_id: u64,
        reviewer_id: Address,
        quality_score: u32,
        technical_issues: Vec<String>,
        repeat_required: bool,
    ) -> Result<(), Error> {
        reviewer_id.require_auth();

        env.storage()
            .instance()
            .get::<DataKey, ImagingStudy>(&DataKey::Study(study_id))
            .ok_or(Error::StudyNotFound)?;

        if quality_score > 100 {
            return Err(Error::InvalidInput);
        }

        let review = QcReview {
            study_id,
            reviewer_id,
            quality_score,
            technical_issues,
            repeat_required,
            reviewed_at: env.ledger().timestamp(),
        };

        env.storage().instance().set(&DataKey::QcReview(study_id), &review);

        Ok(())
    }

    // ── Audit: View Tracking ──────────────────────────────────────────────────

    /// Append an immutable view record for audit purposes.
    /// The viewer must hold a valid (non-expired) access grant,
    /// or be the patient / ordering provider themselves.
    pub fn track_study_views(
        env: Env,
        study_id: u64,
        viewer_id: Address,
        view_timestamp: u64,
        view_duration: u32,
    ) -> Result<(), Error> {
        viewer_id.require_auth();

        env.storage()
            .instance()
            .get::<DataKey, ImagingStudy>(&DataKey::Study(study_id))
            .ok_or(Error::StudyNotFound)?;

        Self::check_access(&env, study_id, &viewer_id)?;

        let view = StudyView {
            study_id,
            viewer_id: viewer_id.clone(),
            view_timestamp,
            view_duration,
        };

        let mut views: Vec<StudyView> = env
            .storage()
            .instance()
            .get(&DataKey::StudyViews(study_id))
            .unwrap_or_else(|| Vec::new(&env));

        views.push_back(view);
        env.storage().instance().set(&DataKey::StudyViews(study_id), &views);

        env.events().publish(
            (Symbol::new(&env, "study_viewed"),),
            (study_id, viewer_id, view_timestamp),
        );

        Ok(())
    }

    // ── Search ────────────────────────────────────────────────────────────────

    /// Return studies for `patient_id` that match `filters`.
    /// `requester` must be the patient or hold a valid grant for each study.
    pub fn search_imaging_studies(
        env: Env,
        patient_id: Address,
        requester: Address,
        filters: ImagingFilters,
    ) -> Result<Vec<ImagingStudy>, Error> {
        requester.require_auth();

        let patient_studies: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::PatientStudies(patient_id.clone()))
            .unwrap_or_else(|| Vec::new(&env));

        let mut results: Vec<ImagingStudy> = Vec::new(&env);

        for i in 0..patient_studies.len() {
            let sid = patient_studies.get_unchecked(i);

            if requester != patient_id
                && Self::check_access(&env, sid, &requester).is_err()
            {
                continue;
            }

            if let Some(study) =
                env.storage()
                    .instance()
                    .get::<DataKey, ImagingStudy>(&DataKey::Study(sid))
            {
                if Self::matches_filters(&study, &filters) {
                    results.push_back(study);
                }
            }
        }

        Ok(results)
    }

    // ── Read helpers ──────────────────────────────────────────────────────────

    pub fn get_study(env: Env, study_id: u64) -> Result<ImagingStudy, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Study(study_id))
            .ok_or(Error::StudyNotFound)
    }

    pub fn get_study_series(env: Env, study_id: u64) -> Vec<ImagingSeries> {
        env.storage()
            .instance()
            .get(&DataKey::Series(study_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_study_views(env: Env, study_id: u64) -> Vec<StudyView> {
        env.storage()
            .instance()
            .get(&DataKey::StudyViews(study_id))
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn get_report(env: Env, study_id: u64) -> Result<ImagingReport, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Report(study_id))
            .ok_or(Error::StudyNotFound)
    }

    pub fn get_qc_review(env: Env, study_id: u64) -> Result<QcReview, Error> {
        env.storage()
            .instance()
            .get(&DataKey::QcReview(study_id))
            .ok_or(Error::StudyNotFound)
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn next_study_id(env: &Env) -> u64 {
        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::StudyCounter)
            .unwrap_or(0u64);
        let next = id + 1;
        env.storage().instance().set(&DataKey::StudyCounter, &next);
        next
    }

    fn next_cd_id(env: &Env) -> u64 {
        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::CdCounter)
            .unwrap_or(0u64);
        let next = id + 1;
        env.storage().instance().set(&DataKey::CdCounter, &next);
        next
    }

    fn push_patient_study(env: &Env, patient_id: &Address, study_id: u64) {
        let mut list: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::PatientStudies(patient_id.clone()))
            .unwrap_or_else(|| Vec::new(env));
        list.push_back(study_id);
        env.storage()
            .instance()
            .set(&DataKey::PatientStudies(patient_id.clone()), &list);
    }

    /// Verify that `viewer_id` may access `study_id`.
    /// The patient and ordering provider always pass; everyone else needs
    /// a valid, non-expired `AccessGrant`.
    fn check_access(env: &Env, study_id: u64, viewer_id: &Address) -> Result<(), Error> {
        let study: ImagingStudy = env
            .storage()
            .instance()
            .get(&DataKey::Study(study_id))
            .ok_or(Error::StudyNotFound)?;

        if viewer_id == &study.patient_id || viewer_id == &study.ordering_provider {
            return Ok(());
        }

        let grant: AccessGrant = env
            .storage()
            .instance()
            .get(&DataKey::AccessGrant(study_id, viewer_id.clone()))
            .ok_or(Error::Unauthorized)?;

        if let Some(exp) = grant.expires_at {
            if env.ledger().timestamp() > exp {
                return Err(Error::AccessExpired);
            }
        }

        Ok(())
    }

    fn matches_filters(study: &ImagingStudy, f: &ImagingFilters) -> bool {
        if let Some(ref m) = f.modality {
            if &study.modality != m {
                return false;
            }
        }
        if let Some(ref bp) = f.body_part {
            if &study.body_part != bp {
                return false;
            }
        }
        if let Some(start) = f.start_date {
            if study.study_date < start {
                return false;
            }
        }
        if let Some(end) = f.end_date {
            if study.study_date > end {
                return false;
            }
        }
        if let Some(cf) = f.has_critical_findings {
            if study.critical_findings != cf {
                return false;
            }
        }
        true
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
