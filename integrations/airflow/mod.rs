use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "airflow",
            "dags",
            "Read DAG states",
            "/api/v1/dags",
            ["limit", "offset"]
        ),
        op!(
            "airflow",
            "runs",
            "Read DAG runs",
            "/api/v1/dags/{dag}/dagRuns",
            ["limit", "order_by"]
        ),
    ]
}
