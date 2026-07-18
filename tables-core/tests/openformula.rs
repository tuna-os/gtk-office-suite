// openformula.rs — spreadsheet function conformance ratchet.
//
// Table-driven cases keyed to the ODF OpenFormula "Small Group" — the
// function set every interoperable spreadsheet must implement — plus the
// most-used Medium Group members. Each case evaluates a formula against a
// fixed grid through TablesEngine (IronCalc) and compares the displayed
// result. The pass count ratchets via corpus/openformula-baseline.txt:
// regressions fail CI, improvements print a bump reminder. Per-function
// results print so gaps are visible in the scorecard age.
//
// Grid fixture: A1=10, A2=20, A3=30, B1=5, B2=-3, B3=2.5,
//               C1="hello", C2="WORLD", C3=" pad ", D1=0

use tables_core::engine::TablesEngine;

fn engine_with_fixture() -> TablesEngine {
    let mut e = TablesEngine::new(30, 10).expect("engine");
    for (r, c, v) in [
        (0, 0, "10"), (1, 0, "20"), (2, 0, "30"),
        (0, 1, "5"), (1, 1, "-3"), (2, 1, "2.5"),
        (0, 2, "hello"), (1, 2, "WORLD"), (2, 2, " pad "),
        (0, 3, "0"),
    ] {
        e.set_cell_text(r, c, v);
    }
    e
}

/// (function name, formula, expected display value)
fn cases() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // ── Math (Small Group) ──────────────────────────────────────────
        ("SUM", "=SUM(A1:A3)", "60"),
        ("SUM-mixed", "=SUM(A1,B1,1)", "16"),
        ("ABS", "=ABS(B2)", "3"),
        ("ROUND", "=ROUND(B3,0)", "3"),
        ("ROUNDDOWN", "=ROUNDDOWN(B3,0)", "2"),
        ("ROUNDUP", "=ROUNDUP(2.1,0)", "3"),
        ("INT", "=INT(B3)", "2"),
        ("MOD", "=MOD(A2,7)", "6"),
        ("SQRT", "=SQRT(25)", "5"),
        ("POWER", "=POWER(2,10)", "1024"),
        ("EXP", "=ROUND(EXP(1),2)", "2.72"),
        ("LN", "=ROUND(LN(EXP(2)),0)", "2"),
        ("LOG", "=LOG(100,10)", "2"),
        ("LOG10", "=LOG10(1000)", "3"),
        ("PI", "=ROUND(PI(),2)", "3.14"),
        ("SIN", "=ROUND(SIN(0),0)", "0"),
        ("COS", "=ROUND(COS(0),0)", "1"),
        ("TAN", "=ROUND(TAN(0),0)", "0"),
        ("TRUNC", "=TRUNC(B3)", "2"),
        ("SIGN", "=SIGN(B2)", "-1"),
        ("PRODUCT", "=PRODUCT(A1,B1)", "50"),
        ("QUOTIENT", "=QUOTIENT(A2,3)", "6"),
        ("GCD", "=GCD(A1,A2)", "10"),
        ("LCM", "=LCM(4,6)", "12"),
        ("EVEN", "=EVEN(3)", "4"),
        ("ODD", "=ODD(4)", "5"),
        ("CEILING", "=CEILING(2.3,1)", "3"),
        ("FLOOR", "=FLOOR(2.7,1)", "2"),
        ("RAND-range", "=IF(AND(RAND()>=0,RAND()<1),1,0)", "1"),
        ("SUMSQ", "=SUMSQ(3,4)", "25"),
        // ── Statistical ─────────────────────────────────────────────────
        ("AVERAGE", "=AVERAGE(A1:A3)", "20"),
        ("MIN", "=MIN(A1:B3)", "-3"),
        ("MAX", "=MAX(A1:B3)", "30"),
        ("COUNT", "=COUNT(A1:B3)", "6"),
        ("COUNTA", "=COUNTA(A1:C3)", "9"),
        ("COUNTBLANK", "=COUNTBLANK(E1:E3)", "3"),
        ("MEDIAN", "=MEDIAN(A1:A3)", "20"),
        ("MODE", "=MODE(1,2,2,3)", "2"),
        ("STDEV", "=ROUND(STDEV(A1:A3),0)", "10"),
        ("VAR", "=VAR(A1:A3)", "100"),
        ("LARGE", "=LARGE(A1:A3,1)", "30"),
        ("SMALL", "=SMALL(A1:A3,2)", "20"),
        ("RANK", "=RANK(A2,A1:A3)", "2"),
        // ── Logical ─────────────────────────────────────────────────────
        ("IF", "=IF(A1>5,\"big\",\"small\")", "big"),
        ("IF-nested", "=IF(A1>100,1,IF(A1>5,2,3))", "2"),
        ("AND", "=IF(AND(A1>5,A2>5),1,0)", "1"),
        ("OR", "=IF(OR(A1>100,A2>5),1,0)", "1"),
        ("NOT", "=IF(NOT(A1>100),1,0)", "1"),
        ("TRUE", "=IF(TRUE(),1,0)", "1"),
        ("FALSE", "=IF(FALSE(),1,0)", "0"),
        ("XOR", "=IF(XOR(TRUE(),FALSE()),1,0)", "1"),
        ("IFERROR", "=IFERROR(1/D1,99)", "99"),
        // ── Text ────────────────────────────────────────────────────────
        ("CONCATENATE", "=CONCATENATE(C1,\" \",C2)", "hello WORLD"),
        ("AMP-concat", "=C1&C2", "helloWORLD"),
        ("LEFT", "=LEFT(C1,3)", "hel"),
        ("RIGHT", "=RIGHT(C1,3)", "llo"),
        ("MID", "=MID(C1,2,3)", "ell"),
        ("LEN", "=LEN(C1)", "5"),
        ("UPPER", "=UPPER(C1)", "HELLO"),
        ("LOWER", "=LOWER(C2)", "world"),
        ("TRIM", "=TRIM(C3)", "pad"),
        ("FIND", "=FIND(\"l\",C1)", "3"),
        ("SEARCH", "=SEARCH(\"L\",C1)", "3"),
        ("SUBSTITUTE", "=SUBSTITUTE(C1,\"l\",\"L\")", "heLLo"),
        ("REPT", "=REPT(\"ab\",3)", "ababab"),
        ("REPLACE", "=REPLACE(C1,1,1,\"J\")", "Jello"),
        ("EXACT", "=IF(EXACT(C1,\"hello\"),1,0)", "1"),
        ("PROPER", "=PROPER(\"war and peace\")", "War And Peace"),
        ("VALUE", "=VALUE(\"42\")+1", "43"),
        ("TEXT", "=TEXT(0.5,\"0%\")", "50%"),
        ("CHAR", "=CHAR(65)", "A"),
        ("CODE", "=CODE(\"A\")", "65"),
        // ── Lookup / reference ──────────────────────────────────────────
        ("VLOOKUP", "=VLOOKUP(20,A1:B3,2,FALSE())", "-3"),
        ("HLOOKUP", "=HLOOKUP(10,A1:B2,2,FALSE())", "20"),
        ("INDEX", "=INDEX(A1:B3,2,1)", "20"),
        ("MATCH", "=MATCH(20,A1:A3,0)", "2"),
        ("CHOOSE", "=CHOOSE(2,\"a\",\"b\",\"c\")", "b"),
        ("OFFSET", "=OFFSET(A1,1,0)", "20"),
        ("ROW", "=ROW(A3)", "3"),
        ("COLUMN", "=COLUMN(B1)", "2"),
        ("ROWS", "=ROWS(A1:A3)", "3"),
        ("COLUMNS", "=COLUMNS(A1:B1)", "2"),
        // ── Info ────────────────────────────────────────────────────────
        ("ISBLANK", "=IF(ISBLANK(E1),1,0)", "1"),
        ("ISNUMBER", "=IF(ISNUMBER(A1),1,0)", "1"),
        ("ISTEXT", "=IF(ISTEXT(C1),1,0)", "1"),
        ("ISEVEN", "=IF(ISEVEN(A1),1,0)", "1"),
        ("ISODD", "=IF(ISODD(3),1,0)", "1"),
        ("ISERROR", "=IF(ISERROR(1/D1),1,0)", "1"),
        ("N", "=N(TRUE())", "1"),
        // ── Date / time ─────────────────────────────────────────────────
        ("DATE-YEAR", "=YEAR(DATE(2026,7,18))", "2026"),
        ("DATE-MONTH", "=MONTH(DATE(2026,7,18))", "7"),
        ("DATE-DAY", "=DAY(DATE(2026,7,18))", "18"),
        ("WEEKDAY", "=WEEKDAY(DATE(2026,7,18),2)", "6"),
        ("DAYS", "=DAYS(DATE(2026,7,18),DATE(2026,7,11))", "7"),
        ("HOUR", "=HOUR(TIME(13,45,30))", "13"),
        ("MINUTE", "=MINUTE(TIME(13,45,30))", "45"),
        ("SECOND", "=SECOND(TIME(13,45,30))", "30"),
        // ── Conditional aggregation ─────────────────────────────────────
        ("COUNTIF", "=COUNTIF(A1:A3,\">15\")", "2"),
        ("SUMIF", "=SUMIF(A1:A3,\">15\")", "50"),
        ("AVERAGEIF", "=AVERAGEIF(A1:A3,\">15\")", "25"),
        ("SUMPRODUCT", "=SUMPRODUCT(A1:A2,B1:B2)", "-10"),
        // ── Financial (Small Group members) ─────────────────────────────
        ("PMT", "=ROUND(PMT(0.05/12,60,-10000),0)", "189"),
        ("FV", "=ROUND(FV(0.05,10,-100),0)", "1258"),
        ("PV", "=ROUND(PV(0.05,10,-100),0)", "772"),
        ("NPER", "=ROUND(NPER(0.05,-100,1000),0)", "14"),
        ("RATE", "=ROUND(RATE(60,-189,10000)*12,2)", "0.05"),
        ("NPV", "=ROUND(NPV(0.1,100,100),0)", "174"),
    ]
}

fn baseline() -> usize {
    include_str!("corpus/openformula-baseline.txt").trim().parse().expect("baseline int")
}

#[test]
fn openformula_conformance_ratchet() {
    let cases = cases();
    let total = cases.len();
    let mut passed = 0usize;
    let mut failures = Vec::new();

    for (name, formula, expected) in &cases {
        let mut e = engine_with_fixture();
        e.set_cell_text(9, 9, formula); // J10 keeps clear of the fixture
        e.evaluate();
        let got = e.cell(9, 9);
        let norm = |s: &str| {
            // display normalization: "3" == "3.00" for numeric comparison
            s.parse::<f64>().map(|v| format!("{v}")).unwrap_or_else(|_| s.to_string())
        };
        if norm(&got) == norm(expected) {
            passed += 1;
        } else {
            failures.push((name, formula, expected, got));
        }
    }

    println!("\nOpenFormula conformance: {passed}/{total}");
    for (name, formula, expected, got) in &failures {
        println!("  FAIL {name:<14} {formula}  want {expected:?} got {got:?}");
    }

    let base = baseline();
    assert!(passed >= base, "REGRESSION: {passed}/{total} below baseline {base}");
    if passed > base {
        println!("IMPROVEMENT: {passed} > baseline {base} — bump tests/corpus/openformula-baseline.txt");
    }
}
