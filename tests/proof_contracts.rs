//! Deterministic proof-obligation and approximation-boundary contracts (LEARN-253).
//!
//! The fixtures in `tests/fixtures/interaction/statistics/proof_contracts.json`
//! record what is an exact identity, a definition, an approximation, or an
//! imported lemma. These tests check the recorded numbers with the standard
//! library only and then check that a recorded explanation can be rejected for
//! crossing an approximation boundary even when its algebra is correct.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Rational {
    num: i128,
    den: i128,
}

impl Rational {
    fn new(num: i128, den: i128) -> Self {
        assert!(den != 0, "rational denominator must be nonzero");
        let sign = if den < 0 { -1 } else { 1 };
        let mut num = num * sign;
        let mut den = den.abs();
        let divisor = gcd(num.unsigned_abs(), den.unsigned_abs());
        if divisor != 0 {
            num /= divisor as i128;
            den /= divisor as i128;
        }
        Self { num, den }
    }

    fn from_pair(pair: (i128, i128)) -> Self {
        Self::new(pair.0, pair.1)
    }

    fn zero() -> Self {
        Self::new(0, 1)
    }

    fn abs(self) -> Self {
        Self::new(self.num.abs(), self.den)
    }

    fn add(self, other: Self) -> Self {
        Self::new(
            self.num * other.den + other.num * self.den,
            self.den * other.den,
        )
    }

    fn mul(self, other: Self) -> Self {
        Self::new(self.num * other.num, self.den * other.den)
    }

    fn sub(self, other: Self) -> Self {
        Self::new(
            self.num * other.den - other.num * self.den,
            self.den * other.den,
        )
    }

    fn div(self, other: Self) -> Self {
        assert!(other.num != 0, "rational division by zero");
        Self::new(self.num * other.den, self.den * other.num)
    }

    fn pow(self, exponent: u32) -> Self {
        let mut result = Self::new(1, 1);
        for _ in 0..exponent {
            result = result.mul(self);
        }
        result
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.num * other.den).cmp(&(other.num * self.den))
    }
}

fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[derive(Debug, Deserialize)]
struct ProofFixture {
    issue_identity: Identity,
    rational_grid: RationalGrid,
    midpoint: Midpoint,
    decomposition: Decomposition,
    contracts: Vec<Contract>,
    recorded_explanations: Vec<RecordedExplanation>,
    missing_link_fixtures: Vec<MissingLinkFixture>,
}

#[derive(Debug, Deserialize)]
struct Identity {
    n: u64,
    p: f64,
    k: u64,
    exact_cdf: f64,
    corrected_phi: f64,
    tolerance: f64,
}

#[derive(Debug, Deserialize)]
struct RationalGrid {
    mu: (i128, i128),
    sigma: (i128, i128),
    spacings: Vec<(i128, i128)>,
    lattice_indices: Vec<i128>,
}

#[derive(Debug, Deserialize)]
struct Midpoint {
    cells: Vec<MidpointCell>,
}

#[derive(Debug, Deserialize)]
struct MidpointCell {
    id: String,
    a: (i128, i128),
    b: (i128, i128),
    #[serde(default)]
    c: (i128, i128),
    h: (i128, i128),
    #[serde(rename = "M")]
    m: i128,
    expected_error: (i128, i128),
    expected_bound: (i128, i128),
}

#[derive(Debug, Deserialize)]
struct Decomposition {
    identity: DecompositionCase,
    accumulation: DecompositionCase,
}

#[derive(Debug, Deserialize)]
struct DecompositionCase {
    n: u64,
    p: f64,
    k: u64,
    tolerance: f64,
}

#[derive(Clone, Debug, Deserialize)]
struct Contract {
    id: String,
    kind: String,
    #[serde(default)]
    proof_supplied: bool,
}

#[derive(Debug, Deserialize)]
struct RecordedExplanation {
    id: String,
    formulas_correct: bool,
    claims: Vec<Claim>,
    expected_verdict: String,
    expected_faults: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Claim {
    contract: String,
    declared_kind: String,
    #[serde(default)]
    asserts_exact: bool,
    #[serde(default)]
    sums_over_expanding_index_set: bool,
    #[serde(default)]
    error_control_declared: Option<String>,
    #[serde(default)]
    discloses_imported_lemma: bool,
}

#[derive(Debug, Deserialize)]
struct MissingLinkFixture {
    id: String,
    domain: String,
    required_links: Vec<(String, String)>,
    recorded_explanations: Vec<LinkExplanation>,
}

#[derive(Debug, Deserialize)]
struct LinkExplanation {
    id: String,
    explained_links: Vec<(String, String)>,
    missing_links: Vec<(String, String)>,
    verdict: String,
}

fn fixture() -> ProofFixture {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/interaction/statistics/proof_contracts.json");
    serde_json::from_slice(&fs::read(path).expect("read proof-contract fixture"))
        .expect("parse proof-contract fixture")
}

fn binomial_coefficient(n: u64, k: u64) -> f64 {
    if k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    let mut result = 1.0_f64;
    for step in 0..k {
        result = result * (n - step) as f64 / (step + 1) as f64;
    }
    result
}

fn exact_binomial_cdf(n: u64, p: f64, k: u64) -> f64 {
    (0..=k)
        .map(|j| binomial_coefficient(n, j) * p.powi(j as i32) * (1.0 - p).powi((n - j) as i32))
        .sum()
}

fn erf(x: f64) -> f64 {
    if x < 0.0 {
        return -erf(-x);
    }
    if x > 3.9 {
        return 1.0;
    }
    let square = x * x;
    let mut term = x;
    let mut sum = x;
    let mut index = 1_u32;
    loop {
        term *= -square / index as f64;
        let contribution = term / (2 * index + 1) as f64;
        sum += contribution;
        if contribution.abs() < 1e-18 * sum.abs().max(1.0) || index > 500 {
            break;
        }
        index += 1;
    }
    2.0 * sum / std::f64::consts::PI.sqrt()
}

fn normal_cdf(z: f64) -> f64 {
    0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))
}

fn midpoint_and_exact(a: Rational, _b: Rational, c: Rational, h: Rational) -> (Rational, Rational) {
    let midpoint = h.mul(c);
    let exact = a.mul(h.pow(3)).div(Rational::new(12, 1)).add(c.mul(h));
    (midpoint, exact)
}

#[test]
fn issue_identity_matches_recorded_exact_and_corrected_values() {
    let fixture = fixture();
    let identity = &fixture.issue_identity;
    assert!((identity.tolerance - 1e-12).abs() < f64::EPSILON);
    let exact = exact_binomial_cdf(identity.n, identity.p, identity.k);
    let sigma = (identity.n as f64 * identity.p * (1.0 - identity.p)).sqrt();
    let corrected = normal_cdf((identity.k as f64 + 0.5 - identity.n as f64 * identity.p) / sigma);
    let uncorrected = normal_cdf((identity.k as f64 - identity.n as f64 * identity.p) / sigma);
    assert!(
        (exact - identity.exact_cdf).abs() < identity.tolerance,
        "exact CDF {exact} differs from recorded {}",
        identity.exact_cdf
    );
    assert!(
        (corrected - identity.corrected_phi).abs() < identity.tolerance,
        "corrected value {corrected} differs from recorded {}",
        identity.corrected_phi
    );
    let corrected_error = (corrected - exact).abs();
    let uncorrected_error = (uncorrected - exact).abs();
    assert!(
        corrected_error < uncorrected_error,
        "the corrected value {corrected} should be closer to the exact {exact} than {uncorrected}"
    );
    assert!(
        corrected_error > 1e-3,
        "the fixture must preserve a remaining gap rather than claim exactness"
    );
}

#[test]
fn rational_grid_keeps_half_cells_units_and_support() {
    let fixture = fixture();
    let grid = &fixture.rational_grid;
    let mu = Rational::from_pair(grid.mu);
    let sigma = Rational::from_pair(grid.sigma);
    assert!(sigma > Rational::zero());
    let expected_half_cells = [
        Rational::new(1, 2),
        Rational::new(1, 1),
        Rational::new(1, 10),
    ];
    let expected_standardized_half_cells = [
        Rational::new(1, 3),
        Rational::new(2, 3),
        Rational::new(1, 15),
    ];
    for ((pair, expected_half_cell), expected_standardized) in grid
        .spacings
        .iter()
        .zip(expected_half_cells)
        .zip(expected_standardized_half_cells)
    {
        let spacing = Rational::from_pair(*pair);
        let half_cell = spacing.div(Rational::new(2, 1));
        assert_eq!(
            half_cell, expected_half_cell,
            "original-coordinate half-cell must be exactly d/2"
        );
        assert_eq!(
            half_cell.mul(Rational::new(2, 1)),
            spacing,
            "two half-cells must tile exactly one spacing"
        );
        let standard_spacing = spacing.div(sigma);
        let standard_half_cell = standard_spacing.div(Rational::new(2, 1));
        assert_eq!(
            standard_half_cell, expected_standardized,
            "standardized half-cell must be h/2 = d/(2 sigma)"
        );
        assert_eq!(
            standard_half_cell.mul(Rational::new(2, 1)),
            standard_spacing,
            "standardized half-cells must tile the standardized spacing"
        );
        let Some(&first) = grid.lattice_indices.first() else {
            panic!("grid fixture must list at least one lattice index");
        };
        let mut previous_coordinate: Option<Rational> = None;
        let mut previous_value: Option<Rational> = None;
        for index in &grid.lattice_indices {
            let value = spacing.mul(Rational::new(*index, 1));
            let coordinate = value.sub(mu).div(sigma);
            if let Some(previous) = previous_coordinate {
                assert_eq!(
                    coordinate.sub(previous),
                    standard_spacing,
                    "standardization must not change lattice spacing"
                );
            }
            if let Some(previous) = previous_value {
                assert_eq!(
                    value.sub(half_cell),
                    previous.add(half_cell),
                    "the inner boundary of a cell is the outer boundary of the previous cell"
                );
            }
            let outer_boundary = value.add(half_cell).sub(mu).div(sigma);
            assert_eq!(
                outer_boundary,
                coordinate.add(standard_half_cell),
                "the standardized outer boundary is z_j + h/2"
            );
            previous_coordinate = Some(coordinate);
            previous_value = Some(value);
        }
        let first_value = spacing.mul(Rational::new(first, 1));
        let next = spacing.mul(Rational::new(first + 1, 1));
        let midpoint = first_value.add(next).div(Rational::new(2, 1));
        assert_eq!(
            midpoint.sub(first_value),
            half_cell,
            "the midpoint of an interval between attainable values is a half-cell away"
        );
        assert_ne!(
            midpoint, first_value,
            "the midpoint between attainable values is not itself attainable"
        );
    }
}

#[test]
fn midpoint_rule_cancels_linear_terms_and_bounds_quadratic_error() {
    let fixture = fixture();
    for cell in &fixture.midpoint.cells {
        let a = Rational::from_pair(cell.a);
        let b = Rational::from_pair(cell.b);
        let c = Rational::from_pair(cell.c);
        let h = Rational::from_pair(cell.h);
        let (midpoint, exact) = midpoint_and_exact(a, b, c, h);
        let error = exact.sub(midpoint).abs();
        let bound = Rational::new(cell.m, 1)
            .mul(h.pow(3))
            .div(Rational::new(24, 1));
        assert_eq!(
            error,
            Rational::from_pair(cell.expected_error),
            "{}: error must match the recorded rational value",
            cell.id
        );
        assert_eq!(
            bound,
            Rational::from_pair(cell.expected_bound),
            "{}: bound must match the recorded rational value",
            cell.id
        );
        assert!(
            error <= bound,
            "{}: midpoint error {error:?} must not exceed the M h^3/24 bound {bound:?}",
            cell.id
        );
        assert_eq!(
            midpoint,
            h.mul(c),
            "{}: the symmetric-cell midpoint value is h times the constant term",
            cell.id
        );
        if cell.id == "linear-cancellation" {
            assert_eq!(
                error,
                Rational::zero(),
                "linear and constant terms must cancel"
            );
            assert_eq!(exact, midpoint, "a linear cell integrates exactly");
        }
        if cell.id == "quadratic-bound" {
            assert_eq!(
                error, bound,
                "a pure quadratic attains the M h^3/24 bound with M = |g''| = 2a"
            );
            let (b_zero_midpoint, b_zero_exact) = midpoint_and_exact(a, Rational::zero(), c, h);
            assert_eq!(
                b_zero_exact, exact,
                "the linear term must not change the exact integral over a symmetric cell"
            );
            assert_eq!(
                b_zero_midpoint, midpoint,
                "the linear term must not change the midpoint value"
            );
            assert_eq!(
                b_zero_exact.sub(b_zero_midpoint).abs(),
                error,
                "the linear coefficient must not change the midpoint error"
            );
        }
    }
}

fn decomposition_terms(case: &DecompositionCase) -> DecompositionNumbers {
    let mu = case.n as f64 * case.p;
    let sigma = (case.n as f64 * case.p * (1.0 - case.p)).sqrt();
    let h = 1.0 / sigma;
    let density = |z: f64| (-z * z / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let masses: Vec<f64> = (0..=case.n)
        .map(|j| {
            binomial_coefficient(case.n, j)
                * case.p.powi(j as i32)
                * (1.0 - case.p).powi((case.n - j) as i32)
        })
        .collect();
    let coordinates: Vec<f64> = (0..=case.n).map(|j| (j as f64 - mu) / sigma).collect();
    let k = case.k as usize;
    let local_mass_error: f64 = (0..=k)
        .map(|j| masses[j] - h * density(coordinates[j]))
        .sum();
    let quadrature_error: f64 = (0..=k)
        .map(|j| {
            let inner = coordinates[j] - h / 2.0;
            let outer = coordinates[j] + h / 2.0;
            h * density(coordinates[j]) - (normal_cdf(outer) - normal_cdf(inner))
        })
        .sum();
    let lower_tail_extension = -normal_cdf(coordinates[0] - h / 2.0);
    let exact = masses[..=k].iter().sum::<f64>();
    let corrected = normal_cdf(coordinates[k] + h / 2.0);
    let residual = exact - corrected - local_mass_error - quadrature_error - lower_tail_extension;
    let max_pointwise = (0..=k)
        .map(|j| (masses[j] - h * density(coordinates[j])).abs())
        .fold(0.0_f64, f64::max);
    DecompositionNumbers {
        exact,
        corrected,
        local_mass_error,
        quadrature_error,
        lower_tail_extension,
        residual,
        max_pointwise,
    }
}

struct DecompositionNumbers {
    exact: f64,
    corrected: f64,
    local_mass_error: f64,
    quadrature_error: f64,
    lower_tail_extension: f64,
    residual: f64,
    max_pointwise: f64,
}

#[test]
fn error_sources_decompose_and_accumulate_separately() {
    let fixture = fixture();
    let identity = decomposition_terms(&fixture.decomposition.identity);
    assert!(
        identity.residual.abs() < fixture.decomposition.identity.tolerance,
        "the three error sources must sum to the total with residual {}",
        identity.residual
    );
    assert!(
        identity.local_mass_error.abs() > 1e-4,
        "the fixture must actually exercise local mass error"
    );
    assert!(
        identity.quadrature_error.abs() > 1e-4,
        "the fixture must actually exercise quadrature error"
    );
    assert!(
        identity.lower_tail_extension.abs() > 1e-6,
        "the fixture must actually exercise the lower-tail extension"
    );

    let accumulation = decomposition_terms(&fixture.decomposition.accumulation);
    assert!(
        accumulation.residual.abs() < fixture.decomposition.accumulation.tolerance,
        "the decomposition identity must still hold for the accumulation fixture"
    );
    assert!(
        accumulation.max_pointwise < 0.01,
        "each pointwise mass difference must be small in the accumulation fixture"
    );
    assert!(
        accumulation.local_mass_error.abs() > accumulation.max_pointwise,
        "the accumulated local mass error {} must exceed the largest single term {}",
        accumulation.local_mass_error.abs(),
        accumulation.max_pointwise
    );
    assert!(
        (accumulation.exact - accumulation.corrected).abs() > accumulation.quadrature_error.abs(),
        "the total error {} must exceed the quadrature error {} alone",
        (accumulation.exact - accumulation.corrected).abs(),
        accumulation.quadrature_error.abs()
    );
    assert!(
        accumulation.lower_tail_extension.abs() > 1e-6,
        "the accumulation fixture must exercise the lower tail too"
    );
}

fn evaluate_explanation(contracts: &[Contract], explanation: &RecordedExplanation) -> Vec<String> {
    let by_id: HashMap<&str, &Contract> = contracts
        .iter()
        .map(|contract| (contract.id.as_str(), contract))
        .collect();
    let mut faults = BTreeSet::new();
    for claim in &explanation.claims {
        let Some(contract) = by_id.get(claim.contract.as_str()) else {
            faults.insert("unknown_contract".to_owned());
            continue;
        };
        if claim.declared_kind != contract.kind {
            faults.insert("declared_kind_mismatch".to_owned());
        }
        if contract.kind == "approximation" {
            if claim.asserts_exact {
                faults.insert("approximation_declared_exact".to_owned());
            }
            if claim.sums_over_expanding_index_set
                && claim.error_control_declared.as_deref() != Some("accumulated_with_tails")
            {
                faults.insert("pointwise_bound_summed_over_index_set".to_owned());
            }
        }
        if contract.kind == "imported_lemma"
            && !contract.proof_supplied
            && !claim.discloses_imported_lemma
        {
            faults.insert("imported_lemma_used_without_disclosure".to_owned());
        }
    }
    faults.into_iter().collect()
}

#[test]
fn recorded_explanations_do_not_silently_cross_approximation_boundaries() {
    let fixture = fixture();
    let kinds: BTreeSet<&str> = fixture
        .contracts
        .iter()
        .map(|contract| contract.kind.as_str())
        .collect();
    for required in [
        "exact_identity",
        "definition",
        "approximation",
        "imported_lemma",
    ] {
        assert!(
            kinds.contains(required),
            "the contract vocabulary must include {required}"
        );
    }
    for explanation in &fixture.recorded_explanations {
        let faults = evaluate_explanation(&fixture.contracts, explanation);
        let expected: BTreeSet<String> = explanation.expected_faults.iter().cloned().collect();
        assert_eq!(
            faults.iter().cloned().collect::<BTreeSet<_>>(),
            expected,
            "{}: contract faults mismatch",
            explanation.id
        );
        let rejected = explanation.expected_verdict == "reject";
        assert_eq!(
            !faults.is_empty(),
            rejected,
            "{}: verdict and faults disagree",
            explanation.id
        );
        if rejected && explanation.formulas_correct {
            assert!(
                !faults.is_empty(),
                "{}: algebraically correct formulas must still fail when a boundary is crossed",
                explanation.id
            );
        }
    }
}

fn missing_links(
    required: &[(String, String)],
    explained: &[(String, String)],
) -> Vec<(String, String)> {
    required
        .iter()
        .filter(|edge| !explained.contains(edge))
        .cloned()
        .collect()
}

#[test]
fn missing_link_trace_gate_rejects_skipped_causal_steps_in_every_domain() {
    let fixture = fixture();
    assert!(
        fixture
            .missing_link_fixtures
            .iter()
            .any(|trace| trace.domain == "code-trace"),
        "the fixture must include a non-mathematical causal trace"
    );
    for trace in &fixture.missing_link_fixtures {
        for explanation in &trace.recorded_explanations {
            let missing = missing_links(&trace.required_links, &explanation.explained_links);
            assert_eq!(
                missing, explanation.missing_links,
                "{}:{}: missing-link gate output mismatch",
                trace.id, explanation.id
            );
            let rejected = explanation.verdict == "reject";
            assert_eq!(
                !missing.is_empty(),
                rejected,
                "{}:{}: verdict and missing links disagree",
                trace.id,
                explanation.id
            );
        }
    }
}

#[test]
fn proof_contract_fixture_is_frozen_and_synthetic() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/interaction/statistics/proof_contracts.json");
    let raw: Value =
        serde_json::from_slice(&fs::read(&path).expect("read raw proof-contract fixture"))
            .expect("parse raw proof-contract fixture");
    assert_eq!(raw["synthetic"], true);
    assert_eq!(raw["private_material"], false);
    assert_eq!(raw["fixture_version"], "statistics/proof-contracts/v1");
    assert_eq!(raw["issue_identity"]["tolerance"], 1e-12);
}
