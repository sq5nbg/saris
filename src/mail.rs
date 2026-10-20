//! Confirmation e-mails sent to students after a successful upload.

use std::time::Duration;

use lettre::message::{Mailbox, header::ContentType};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::{SmtpConfig, SmtpTls};
use crate::upload::Student;

/// Overall time budget for sending all confirmation messages.
const SEND_TIMEOUT: Duration = Duration::from_secs(15);

/// Data describing the accepted upload, used in the e-mail body.
pub struct Confirmation<'a> {
    pub subject: &'a str,
    pub title: &'a str,
    pub doc_type: &'a str,
    pub stored_name: &'a str,
    pub original_name: &'a str,
    pub received_at: &'a str,
    pub students: &'a [Student],
}

fn build_transport(cfg: &SmtpConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
    let builder = match cfg.tls {
        SmtpTls::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host),
        SmtpTls::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host),
        SmtpTls::None => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
            &cfg.host,
        )),
        SmtpTls::Auto if cfg.port == 465 => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host),
        SmtpTls::Auto => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host),
    }
    .map_err(|e| format!("SMTP transport error: {e}"))?;
    let mut builder = builder.port(cfg.port).timeout(Some(SEND_TIMEOUT));
    if !cfg.username.is_empty() {
        builder = builder.credentials(Credentials::new(cfg.username.clone(), cfg.password.clone()));
    }
    Ok(builder.build())
}

fn body(c: &Confirmation<'_>) -> String {
    let students = c
        .students
        .iter()
        .map(|s| format!("  - {} {} <{}>", s.first, s.last, s.email))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Dzień dobry,\n\n\
         system SARIS potwierdza przyjęcie pliku.\n\n\
         Przedmiot: {}\n\
         Tytuł ćwiczenia: {}\n\
         Typ dokumentu: {}\n\
         Przesłany plik: {}\n\
         Zapisano jako: {}\n\
         Data odbioru: {}\n\
         Studenci:\n{}\n\n\
         Wiadomość wygenerowana automatycznie - prosimy na nią nie odpowiadać.\n\
         SARIS - System Archiwizacji Raportów i Sprawozdań\n",
        c.subject, c.title, c.doc_type, c.original_name, c.stored_name, c.received_at, students
    )
}

async fn send_all(cfg: &SmtpConfig, c: &Confirmation<'_>) -> Result<(), String> {
    let transport = build_transport(cfg)?;
    let from: Mailbox = cfg
        .from
        .parse()
        .map_err(|e| format!("invalid smtp.from address: {e}"))?;
    let text = body(c);
    let mut failures = Vec::new();
    for s in c.students {
        let to = match s.email.parse() {
            Ok(addr) => Mailbox::new(Some(format!("{} {}", s.first, s.last)), addr),
            Err(e) => {
                failures.push(format!("{}: {e}", s.email));
                continue;
            }
        };
        let msg = Message::builder()
            .from(from.clone())
            .to(to)
            .subject(format!(
                "SARIS: potwierdzenie przyjęcia pliku ({} - {})",
                c.subject, c.title
            ))
            .header(ContentType::TEXT_PLAIN)
            .body(text.clone());
        match msg {
            Ok(m) => {
                if let Err(e) = transport.send(m).await {
                    failures.push(format!("{}: {e}", s.email));
                }
            }
            Err(e) => failures.push(format!("{}: {e}", s.email)),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// Send a confirmation to every student. Returns `true` only if all
/// messages were accepted by the SMTP server within the time budget.
/// Never fails the upload: errors are logged via `tracing`.
pub async fn send_confirmations(cfg: &SmtpConfig, c: &Confirmation<'_>) -> bool {
    if !cfg.enabled || c.students.is_empty() {
        return false;
    }
    match tokio::time::timeout(SEND_TIMEOUT, send_all(cfg, c)).await {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            tracing::warn!("confirmation e-mail failed: {e}");
            false
        }
        Err(_) => {
            tracing::warn!("confirmation e-mail timed out after {SEND_TIMEOUT:?}");
            false
        }
    }
}
