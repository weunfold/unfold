

use crate::policy::Kind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Pt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reason {
    kind: Kind,

    days: u32,
}

impl Reason {
    pub fn name_wish() -> Reason {
        Reason { kind: Kind::NameWish, days: 0 }
    }

    pub fn overnight() -> Reason {
        Reason { kind: Kind::Overnight, days: 0 }
    }

    pub fn curiosity() -> Reason {
        Reason { kind: Kind::Curiosity, days: 0 }
    }

    pub fn open_threads(days: u32) -> Reason {
        Reason { kind: Kind::OpenThreads, days: days.max(1) }
    }

    pub(crate) fn of(kind: Kind, days: u32) -> Reason {
        match kind {
            Kind::OpenThreads => Reason::open_threads(days),
            _ => Reason { kind, days: 0 },
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn render(&self, lang: Lang) -> String {
        let n = self.days;
        match (self.kind, lang) {
            (Kind::NameWish, Lang::En) => "Why now: I have a name idea waiting for your OK.".into(),
            (Kind::NameWish, Lang::Pt) => "Por que agora: tenho uma ideia de nome esperando seu OK.".into(),
            (Kind::Overnight, Lang::En) => "Why now: this came out of last night's reflection.".into(),
            (Kind::Overnight, Lang::Pt) => "Por que agora: isso veio da reflexão de ontem à noite.".into(),
            (Kind::Curiosity, Lang::En) => "Why now: this has been on my mind since we talked.".into(),
            (Kind::Curiosity, Lang::Pt) => "Por que agora: isso ficou na minha cabeça desde a nossa conversa.".into(),
            (Kind::OpenThreads, Lang::En) if n == 1 => "Why now: this thread has been open for 1 day.".into(),
            (Kind::OpenThreads, Lang::En) => {
                format!("Why now: this thread has been open for {n} days.")
            }
            (Kind::OpenThreads, Lang::Pt) if n == 1 => "Por que agora: esta conversa está aberta há 1 dia.".into(),
            (Kind::OpenThreads, Lang::Pt) => {
                format!("Por que agora: esta conversa está aberta há {n} dias.")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub title: String,
    pub text: String,
    reason: Reason,
}

impl Message {
    pub fn new(title: String, text: String, reason: Reason) -> Message {
        Message { title, text, reason }
    }

    pub fn reason(&self) -> &Reason {
        &self.reason
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<(Reason, String, String)> {
        [Reason::name_wish(), Reason::overnight(), Reason::curiosity(), Reason::open_threads(1), Reason::open_threads(3)].into_iter().map(|r| (r, r.render(Lang::En), r.render(Lang::Pt))).collect()
    }

    #[test]
    fn b3_each_kind_renders_its_settled_reason_line() {
        let lines: Vec<(String, String)> = all().into_iter().map(|(_, en, pt)| (en, pt)).collect();
        let want = [
            ("Why now: I have a name idea waiting for your OK.", "Por que agora: tenho uma ideia de nome esperando seu OK."),
            ("Why now: this came out of last night's reflection.", "Por que agora: isso veio da reflexão de ontem à noite."),
            ("Why now: this has been on my mind since we talked.", "Por que agora: isso ficou na minha cabeça desde a nossa conversa."),
            ("Why now: this thread has been open for 1 day.", "Por que agora: esta conversa está aberta há 1 dia."),
            ("Why now: this thread has been open for 3 days.", "Por que agora: esta conversa está aberta há 3 dias."),
        ];
        assert_eq!(lines, want.map(|(e, p)| (e.to_string(), p.to_string())));
        assert_eq!(Reason::open_threads(0).render(Lang::En), Reason::open_threads(1).render(Lang::En), "never 0 days");
        assert_eq!(Message::new("Lua".into(), "hi".into(), Reason::overnight()).reason().kind(), Kind::Overnight);
    }

    #[test]
    fn b3_portuguese_reason_lines_use_no_article_and_no_gendered_word() {
        for (_, _, pt) in all() {
            let lower = pt.to_lowercase();
            for n in ["unfold", "lua"] {
                for article in ["o", "a", "do", "da", "no", "na", "ao", "à", "pelo", "pela"] {
                    assert!(!lower.contains(&format!(" {article} {n}")), "{article} {n}: {pt}");
                }
            }
            for gendered in
                ["criado", "criada", "feito", "feita", " ele ", " ela ", "dele", "dela", "obrigado", "obrigada", "curioso", "curiosa", "animado", "animada", "pronto", "pronta", "sozinho", "sozinha"]
            {
                assert!(!lower.contains(gendered), "{gendered}: {pt}");
            }
            assert!(lower.starts_with("por que agora: "), "{pt}");
        }
    }
}
