use super::Operation;

pub fn operations() -> Vec<Operation> {
    vec![
        op!(
            "rabbitmq",
            "queues",
            "Read queue depth and consumers",
            "/api/queues/{vhost}",
            ["page", "page_size"]
        ),
        op!(
            "rabbitmq",
            "overview",
            "Read broker health",
            "/api/overview",
            []
        ),
        op!(
            "rabbitmq",
            "connections",
            "Read broker connections",
            "/api/connections",
            ["page", "page_size"]
        ),
    ]
}
