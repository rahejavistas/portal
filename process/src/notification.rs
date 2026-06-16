use uuid::Uuid;

/// Notify admin that a step is under review or requires attention.
pub fn notify_admin(
    process_id: Uuid,
    user_id: i32,
    template_step_id: i32,
    step_name: &str,
    details: &str,
) {
    // The function can decide what to do with the details (e.g., log, push to a queue, send email, etc.)
    // based on the step which got reviewed, completed, or requires attention.
    println!(
        "[NOTIFICATION] [ADMIN] Process: {}, User ID: {}, Step ID: {} ({}) - Details: {}",
        process_id, user_id, template_step_id, step_name, details
    );
}

/// Notify the user that a step requires their action (it's their turn) or has been updated.
pub fn notify_user(
    process_id: Uuid,
    user_id: i32,
    template_step_id: i32,
    step_name: &str,
    details: &str,
) {
    // The function can decide what to do with the details (e.g., log, push to a queue, send email, etc.)
    // based on the step which got reviewed, completed, or requires attention.
    println!(
        "[NOTIFICATION] [USER] Process: {}, User ID: {}, Step ID: {} ({}) - Details: {}",
        process_id, user_id, template_step_id, step_name, details
    );
}
