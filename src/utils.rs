use crate::config::EmailNoticeConfig;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::header::ContentType,
    transport::smtp::authentication::Credentials,
};

/// 发送邮件
pub(crate) async fn send_email(config: EmailNoticeConfig, user_id: &str) -> anyhow::Result<()> {
    let from = format!("Bot <{}>", config.username);
    let to = format!("Master <{}>", config.receiver);
    let email_template = config.mail.clone().unwrap_or_default();
    let body = replace_placeholders(&email_template.body, &[("bot_id", user_id)]);

    let email = Message::builder()
        .from(from.parse()?)
        .to(to.parse()?)
        .subject(email_template.subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body)?;

    let creds = Credentials::new(config.username.clone(), config.password.clone());

    // Open a remote connection to gmail
    let mailer: AsyncSmtpTransport<Tokio1Executor> = AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp)?
        .credentials(creds)
        .build();

    mailer.send(email).await?;

    Ok(())
}

fn replace_placeholders(text: &str, map: &[(&str, &str)]) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;
    let mut placeholder_start = None;

    for (i, c) in text.char_indices() {
        match c {
            '{' => placeholder_start = Some(i),
            '}' => {
                if let Some(start) = placeholder_start {
                    let key = &text[start + 1..i];
                    if let Some((_, value)) = map.iter().find(|&&(k, _)| k == key) {
                        result.push_str(&text[last_end..start]);
                        result.push_str(value);
                        last_end = i + 1;
                    }
                    placeholder_start = None;
                }
            }
            _ => {}
        }
    }
    result.push_str(&text[last_end..]);
    result
}
