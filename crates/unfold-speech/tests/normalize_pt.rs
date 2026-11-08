use unfold_speech::normalize_pt;

fn check(cases: &[(&str, &str)]) {
    let bad: Vec<String> = cases
        .iter()
        .filter_map(|(input, want)| {
            let got = normalize_pt(input);
            (got != *want).then(|| format!("{input:?}\n   want {want:?}\n    got {got:?}"))
        })
        .collect();
    assert!(bad.is_empty(), "{} of {} wrong:\n{}", bad.len(), cases.len(), bad.join("\n"));
}

#[test]
fn pt04_reads_money_as_reais_and_centavos() {
    check(&[("Dá R$ 42,50 para vocês dois.", "Dá quarenta e dois reais e cinquenta centavos para vocês dois.")]);
}

#[test]
fn money() {
    check(&[
        ("R$ 1,00", "um real"),
        ("R$ 0,50", "cinquenta centavos"),
        ("R$ 0,01", "um centavo"),
        ("R$ 1,01", "um real e um centavo"),
        ("R$ 0,00", "zero reais"),
        ("R$ 2", "dois reais"),
        ("R$42,50", "quarenta e dois reais e cinquenta centavos"),
        ("R$\u{a0}42,50", "quarenta e dois reais e cinquenta centavos"),
        ("R$ 1.234,56", "mil duzentos e trinta e quatro reais e cinquenta e seis centavos"),
        ("R$ 1.000.000,00", "um milhão de reais"),
        ("R$ 2.000.000,50", "dois milhões de reais e cinquenta centavos"),
        ("R$ 200,00", "duzentos reais"),
        ("Custa R$ 9,90.", "Custa nove reais e noventa centavos."),
    ]);
}

#[test]
fn integers() {
    check(&[
        ("0", "zero"),
        ("1", "um"),
        ("15", "quinze"),
        ("100", "cem"),
        ("101", "cento e um"),
        ("999", "novecentos e noventa e nove"),
        ("1000", "mil"),
        ("1001", "mil e um"),
        ("1100", "mil e cem"),
        ("1101", "mil cento e um"),
        ("2026", "dois mil e vinte e seis"),
        ("1.200.000", "um milhão e duzentos mil"),
        ("1.200.300", "um milhão duzentos mil e trezentos"),
        ("2.345.678", "dois milhões trezentos e quarenta e cinco mil seiscentos e setenta e oito"),
        ("1.000.000.001", "um bilhão e um"),
    ]);
}

#[test]
fn thousands_and_decimals() {
    check(&[
        ("1.234,56", "mil duzentos e trinta e quatro vírgula cinquenta e seis"),
        ("3,5", "três vírgula cinco"),
        ("3,05", "três vírgula zero cinco"),
        ("0,75", "zero vírgula setenta e cinco"),
        ("12.000", "doze mil"),
    ]);
}

#[test]
fn percent() {
    check(&[
        ("50%", "cinquenta por cento"),
        ("1,5%", "um vírgula cinco por cento"),
        ("100%", "cem por cento"),
        ("Subiu 2% hoje.", "Subiu dois por cento hoje."),
    ]);
}

#[test]
fn gender_agreement() {
    check(&[
        ("2 pessoas", "duas pessoas"),
        ("1 pessoa", "uma pessoa"),
        ("21 pessoas", "vinte e uma pessoas"),
        ("200 pessoas", "duzentas pessoas"),
        ("2.000 pessoas", "duas mil pessoas"),
        ("1 hora", "uma hora"),
        ("2 vezes", "duas vezes"),
        ("2 opções", "duas opções"),
        ("2 cidades", "duas cidades"),
        ("2 dias", "dois dias"),
        ("2 problemas", "dois problemas"),
        ("2 reais", "dois reais"),
        ("2 amigos", "dois amigos"),
    ]);
}

#[test]
fn sentence_punctuation_stays() {
    check(&[
        ("Custa 1.000.", "Custa mil."),
        ("1, 2 e 3.", "um, dois e três."),
        ("São 3,5.", "São três vírgula cinco."),
    ]);
}

#[test]
fn unknown_shapes_pass_through() {
    for s in [
        "14:30",
        "27/10/2026",
        "versão 0.4.8",
        "1.5",
        "1.23",
        "R$ 4,5",
        "R$ 1,234",
        "3h",
        "10x",
        "5km",
        "2º lugar",
        "abc123",
        "11987654321",
        "12345",
        "007",
        "CEP 01310-100",
        "-5",
        "R$ -5",
        "#3",
        "Olá, tudo bem?",
        "",
    ] {
        assert_eq!(normalize_pt(s), s, "{s:?} must pass through");
    }
}

#[test]
fn idempotent() {
    for s in ["Dá R$ 42,50 para vocês dois.", "2 pessoas e 50%", "1.234,56", "14:30"] {
        let once = normalize_pt(s);
        assert_eq!(normalize_pt(&once), once);
    }
}

#[test]
fn h1_amount_before_a_scale_word() {
    check(&[
        ("R$ 2 mil", "dois mil reais"),
        ("R$ 10 mil", "dez mil reais"),
        ("R$ 3 milhões", "três milhões de reais"),
        ("R$ 1 milhão", "um milhão de reais"),
        ("R$ 2 mil reais", "dois mil reais"),
        ("R$ 1 bilhão de reais", "um bilhão de reais"),
        ("Custou R$ 5 mil.", "Custou cinco mil reais."),
        ("R$ 1,5 bilhão", "R$ 1,5 bilhão"),
    ]);
}

#[test]
fn h2_gender_looks_past_a_spelled_mil() {
    check(&[("2 mil pessoas", "duas mil pessoas"), ("200 mil pessoas", "duzentas mil pessoas"), ("2 mil dias", "dois mil dias")]);
}

#[test]
fn h3_function_words_verbs_and_labels_stay_masculine() {
    check(&[
        ("de 1 a 5 dias", "de um a cinco dias"),
        ("Ganhou de 2 a 1", "Ganhou de dois a um"),
        ("2 contra 1", "dois contra um"),
        ("2 para 1", "dois para um"),
        ("Faltam 2 para acabar", "Faltam dois para acabar"),
        ("1 na lista", "um na lista"),
        ("1 cada", "um cada"),
        ("o plano 2 funciona", "o plano dois funciona"),
        ("o 2 ganha", "o dois ganha"),
        ("Fase 1 completa", "Fase um completa"),
        ("página 2 da apostila", "página dois da apostila"),
        ("1 da manhã", "uma da manhã"),
        ("2 das pessoas", "duas das pessoas"),
    ]);
}

#[test]
fn h4_feminine_classes() {
    check(&[
        ("2 versões", "duas versões"),
        ("2 sessões", "duas sessões"),
        ("2 decisões", "duas decisões"),
        ("2 questões", "duas questões"),
        ("2 regiões", "duas regiões"),
        ("2 razões", "duas razões"),
        ("2 opiniões", "duas opiniões"),
        ("2 mãos", "duas mãos"),
        ("2 fotos", "duas fotos"),
        ("2 leis", "duas leis"),
        ("2 vozes", "duas vozes"),
        ("2 luzes", "duas luzes"),
        ("2 classes", "duas classes"),
        ("2 frases", "duas frases"),
        ("2 fases", "duas fases"),
        ("2 nuvens", "duas nuvens"),
        ("2 árvores", "duas árvores"),
        ("2 colheres", "duas colheres"),
        ("2 paredes", "duas paredes"),
        ("2 artes", "duas artes"),
        ("2 corações", "dois corações"),
        ("2 viagens", "duas viagens"),
        ("2 semanas", "duas semanas"),
        ("2 horas", "duas horas"),
    ]);
}

#[test]
fn h5_masculine_nouns_in_a() {
    check(&[
        ("2 sintomas", "dois sintomas"),
        ("2 dilemas", "dois dilemas"),
        ("2 dramas", "dois dramas"),
        ("2 cometas", "dois cometas"),
        ("2 quilogramas", "dois quilogramas"),
        ("2 motoristas", "dois motoristas"),
        ("200 gramas", "duzentos gramas"),
        ("2 formas", "duas formas"),
        ("2 camas", "duas camas"),
        ("2 turmas", "duas turmas"),
    ]);
}

#[test]
fn h6_currency_after_punctuation() {
    check(&[
        ("Total:R$ 10,00", "Total:dez reais"),
        ("Total: R$ 10,00", "Total: dez reais"),
        ("(R$ 5)", "(cinco reais)"),
        ("-R$ 5,00", "-R$ 5,00"),
        ("R$ 10,00-R$ 20,00", "R$ 10,00-R$ 20,00"),
        ("US$ 10", "US$ 10"),
        ("€10", "€10"),
        ("r$ 10", "dez reais"),
    ]);
}

#[test]
fn m1_space_joined_digit_groups_pass_through() {
    for s in ["0800 123 4567", "11 98765 4321", "4111 1111 1111 1111", "(11) 98765-4321", "+55 11 98765-4321"] {
        assert_eq!(normalize_pt(s), s, "{s:?} must pass through");
    }
}

#[test]
fn m2_dot_before_a_letter_is_part_of_the_token() {
    for s in ["arquivo 2.txt", "relatório 2026.pdf", "item 3.a", "1.º lugar"] {
        assert_eq!(normalize_pt(s), s, "{s:?} must pass through");
    }
}

#[test]
fn m3_addresses_and_versions_pass_through() {
    for s in ["200.100.100.100", "versão 4.100", "v 2.300", "version 1.200"] {
        assert_eq!(normalize_pt(s), s, "{s:?} must pass through");
    }
    check(&[("12.000 pessoas", "doze mil pessoas")]);
}

#[test]
fn m4_de_after_a_round_million_before_a_noun() {
    check(&[
        ("2.000.000 pessoas", "dois milhões de pessoas"),
        ("1.000.000 pessoas", "um milhão de pessoas"),
        ("1.000.000 de pessoas", "um milhão de pessoas"),
        ("1.200.000 pessoas", "um milhão e duzentas mil pessoas"),
    ]);
}

#[test]
fn m5_first_of_the_month() {
    check(&[("1 de maio", "primeiro de maio"), ("1º de maio", "1º de maio"), ("2 de maio", "dois de maio"), ("1 de muitos", "um de muitos")]);
}

#[test]
fn m6_code_urls_and_markup_pass_through() {
    for s in [
        "`foo(10)`",
        "```\nlet x = 10;\n```",
        "array[0]",
        "?page=2",
        "a=1",
        "1+1",
        "2*3",
        "veja https://exemplo.com/2/pagina?x=3 agora",
        "www.site.com.br/10",
        "<pause> ok",
        "⟨mood=calm⟩ oi",
    ] {
        assert_eq!(normalize_pt(s), s, "{s:?} must pass through");
    }
    check(&[
        ("Use `x=10` e depois 2 pessoas.", "Use `x=10` e depois duas pessoas."),
        ("<laugh> 2 pessoas", "<laugh> duas pessoas"),
        ("veja www.a.com e 3 dias", "veja www.a.com e três dias"),
    ]);
}

#[test]
fn lows_percent_dashes_and_digit_cap() {
    check(&[("50 %", "cinquenta por cento"), ("50%off", "50%off"), ("5%5", "5%5"), ("10–20", "10–20"), ("12345,5", "12345,5")]);
}

#[test]
fn fuzz_idempotent_and_no_panic() {
    let alphabet: Vec<char> = "0123456789.,%R$r -/:()a pessoasmilº°\u{a0}éçãõ+=[]*`<>⟨⟩\nhttps://www.".chars().collect();
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..50_000 {
        let mut s = String::new();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        for _ in 0..(x % 24) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            s.push(alphabet[(x % alphabet.len() as u64) as usize]);
        }
        let once = normalize_pt(&s);
        assert_eq!(normalize_pt(&once), once, "not idempotent on {s:?}");
    }
}

#[test]
fn long_inputs_stay_fast() {
    for s in ["`".repeat(50_000), "R$".repeat(50_000), "1.".repeat(50_000), "<a".repeat(50_000), "9".repeat(100_000)] {
        let t = std::time::Instant::now();
        let _ = normalize_pt(&s);
        assert!(t.elapsed().as_secs_f64() < 2.0, "slow on a {}-char input", s.len());
    }
}

#[test]
fn r2_article_before_the_number_decides() {
    check(&[
        ("Os 2 estão prontos", "Os dois estão prontos"),
        ("2 ESTÃO", "dois ESTÃO"),
        ("o 1 volta amanhã", "o um volta amanhã"),
        ("umas 2 horas", "umas duas horas"),
        ("as 2 melhores", "as duas melhores"),
        ("na 1 e na 2", "na uma e na duas"),
        ("dos 2 lados", "dos dois lados"),
        ("de 1 a 2 dias", "de um a dois dias"),
        ("Ganhou de 2 a 1", "Ganhou de dois a um"),
        ("às 2 da tarde", "às duas da tarde"),
        ("chegou a 2 graus", "chegou a dois graus"),
        ("a 2 pessoas", "a duas pessoas"),
        ("os 2 milhões", "os dois milhões"),
    ]);
}

#[test]
fn r2_verbs_in_gem_and_stao_never_count() {
    check(&[
        ("Os 2 agem rápido", "Os dois agem rápido"),
        ("2 reagem", "dois reagem"),
        ("2 exigem", "dois exigem"),
        ("2 surgem", "dois surgem"),
        ("2 estão", "dois estão"),
        ("2 bastões", "dois bastões"),
        ("1 imagem", "uma imagem"),
        ("2 viagens", "duas viagens"),
        ("1 origem", "uma origem"),
        ("2 mensagens", "duas mensagens"),
        ("2 garagens", "duas garagens"),
        ("2 questões", "duas questões"),
        ("2 personagens", "dois personagens"),
        ("2 versões", "duas versões"),
        ("2 cidades", "duas cidades"),
    ]);
}

#[test]
fn r2_more_scale_words_after_an_amount() {
    check(&[
        ("R$ 1 trilhão", "um trilhão de reais"),
        ("R$ 2 trilhões", "dois trilhões de reais"),
        ("R$ 500 mi", "R$ 500 mi"),
        ("R$ 2 bi", "R$ 2 bi"),
        ("R$ 3 tri", "R$ 3 tri"),
        ("R$ 2 milhares", "R$ 2 milhares"),
    ]);
}

#[test]
fn r2_de_only_before_a_listed_noun() {
    check(&[
        ("Foram 2.000.000 ontem", "Foram dois milhões ontem"),
        ("Somos 1.000.000 hoje", "Somos um milhão hoje"),
        ("2.000.000 e 3", "dois milhões e três"),
        ("1.000.000 já", "um milhão já"),
        ("2.000.000 pessoas", "dois milhões de pessoas"),
        ("3.000.000 habitantes", "três milhões de habitantes"),
        ("2.000.000 visualizações", "dois milhões de visualizações"),
        ("1.000.000 de pessoas", "um milhão de pessoas"),
    ]);
}

#[test]
fn r2_unclosed_angle_quote_is_linear() {
    let s = "⟨".repeat(50_000);
    let t = std::time::Instant::now();
    assert_eq!(normalize_pt(&s), s);
    assert!(t.elapsed().as_secs_f64() < 0.5, "slow: {:?}", t.elapsed());
}

#[test]
fn r3_times_after_a_and_as() {
    check(&[
        ("Te vejo às 2.", "Te vejo às duas."),
        ("às 2 e meia", "às duas e meia"),
        ("à 1 h", "à uma h"),
        ("às 3 da tarde", "às três da tarde"),
        ("às 2 da noite", "às duas da noite"),
        ("às 2 pessoas", "às duas pessoas"),
        ("às 2 dias", "às dois dias"),
        ("de 1 a 2.", "de um a dois."),
        ("de 1 à 2.", "de um à dois."),
        ("à 1.", "à uma."),
    ]);
}

#[test]
fn r3_de_before_plural_masculine_nouns() {
    check(&[
        ("2.000.000 reais", "dois milhões de reais"),
        ("3.000.000 meses", "três milhões de meses"),
        ("1.000.000 países", "um milhão de países"),
        ("2.000.000 barris", "dois milhões de barris"),
    ]);
}
