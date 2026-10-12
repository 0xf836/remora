#[test]
fn full_admission_and_application_queue_budgets_preserve_admitted_probes() {
    let summary = super::concurrency::run_case("overload", 5, 32);
    for key in ["within_one_second_ratio", "reverse_within_one_second_ratio"] {
        for ratio in summary[key].as_array().unwrap() {
            assert!(ratio.as_f64().unwrap() >= 0.99);
        }
    }
}
