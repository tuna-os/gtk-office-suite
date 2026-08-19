// templates.rs — built-in templates and template metadata.
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemplateKind {
    Letters,
    Tables,
    Decks,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub kind: TemplateKind,
    pub icon_name: &'static str,
    pub content: &'static str,
}

pub const LETTERS_TEMPLATES: &[DocumentTemplate] = &[
    DocumentTemplate {
        id: "letters-blank",
        name: "Blank Document",
        description: "Start from a clean slate with default styling.",
        kind: TemplateKind::Letters,
        icon_name: "document-new-symbolic",
        content: "",
    },
    DocumentTemplate {
        id: "letters-formal-letter",
        name: "Formal Letter",
        description: "Standard business letter with sender/recipient blocks and closing.",
        kind: TemplateKind::Letters,
        icon_name: "mail-send-symbolic",
        content: "# Sender Name\n123 Street Road\nCity, State 12345\n\nDate: August 18, 2026\n\n**Recipient Name**\nCompany Inc.\n456 Avenue Way\n\nDear Recipient,\n\nI am writing to you regarding the project roadmap and milestones for the upcoming quarter.\n\nThank you for your time and consideration.\n\nSincerely,\n\nSender Name\n",
    },
    DocumentTemplate {
        id: "letters-report",
        name: "Project Report",
        description: "Executive summary, background, methodology, and recommendations.",
        kind: TemplateKind::Letters,
        icon_name: "x-office-document-symbolic",
        content: "# Project Status & Executive Report\n\n## 1. Executive Summary\n\nThis document summarizes the current status, deliverables, and upcoming milestones.\n\n## 2. Key Objectives\n\n- Deliver high-fidelity desktop editing workflows\n- Ensure complete interoperability with open and industry standards\n- Maintain full privacy and accessibility compliance\n\n## 3. Findings and Next Steps\n\nAll targets for the milestone have been met.\n",
    },
    DocumentTemplate {
        id: "letters-resume",
        name: "Resume / CV",
        description: "Clean modern curriculum vitae layout.",
        kind: TemplateKind::Letters,
        icon_name: "contact-new-symbolic",
        content: "# Jane Developer\n\nemail@example.com · +1 (555) 012-3456 · github.com/developer\n\n## Experience\n\n**Senior Systems Engineer** — TunaOS (2024–Present)\n- Architected cross-platform desktop productivity suite with Rust and GTK4.\n- Implemented robust document processing engines and accessible GUI components.\n\n## Education\n\n**B.S. in Computer Science** — University of Technology (2020–2024)\n\n## Skills\n- Rust, GTK4, Libadwaita, Python, Linux Systems\n",
    },
];

pub const TABLES_TEMPLATES: &[DocumentTemplate] = &[
    DocumentTemplate {
        id: "tables-blank",
        name: "Blank Workbook",
        description: "New empty spreadsheet.",
        kind: TemplateKind::Tables,
        icon_name: "x-office-spreadsheet-symbolic",
        content: "",
    },
    DocumentTemplate {
        id: "tables-budget",
        name: "Monthly Budget",
        description: "Income, expenses, and net balance calculation sheet.",
        kind: TemplateKind::Tables,
        icon_name: "view-list-bullet-symbolic",
        content: "Category,Planned,Actual,Difference\nIncome,5000,5200,=C2-B2\nRent,1500,1500,=C3-B3\nGroceries,400,380,=C4-B4\nUtilities,200,210,=C5-B5\nTransportation,150,140,=C6-B6\nTotal Expenses,=SUM(B3:B6),=SUM(C3:C6),=C7-B7\nNet Income,=B2-B7,=C2-C7,=C8-B8\n",
    },
    DocumentTemplate {
        id: "tables-invoice",
        name: "Simple Invoice",
        description: "Itemized billing with unit pricing, quantity, and taxes.",
        kind: TemplateKind::Tables,
        icon_name: "emblem-documents-symbolic",
        content: "Item,Description,Quantity,Unit Price,Total\n1,Consulting Services,40,120,=C2*D2\n2,System Implementation,20,150,=C3*D3\n3,Documentation & Training,10,100,=C4*D4\nSubtotal,,,,=SUM(E2:E4)\nTax (10%),,,,=E5*0.1\nTotal Due,,,,=E5+E6\n",
    },
];

pub const DECKS_TEMPLATES: &[DocumentTemplate] = &[
    DocumentTemplate {
        id: "decks-blank",
        name: "Blank Presentation",
        description: "Clean presentation deck.",
        kind: TemplateKind::Decks,
        icon_name: "x-office-presentation-symbolic",
        content: "# Title Slide\n## Subtitle goes here\n",
    },
    DocumentTemplate {
        id: "decks-pitch",
        name: "Pitch Deck",
        description: "Problem, Solution, Market, Traction, and Team slides.",
        kind: TemplateKind::Decks,
        icon_name: "view-paged-symbolic",
        content: "# Product Vision & Pitch\n## Changing the way desktop software is built\n---\n# The Problem\n- Legacy office suites are bloated and brittle\n- Fragmented user experience across platforms\n---\n# The Solution\n- Fast, safe, accessible desktop office suite in Rust\n- Native GNOME integration with full interoperability\n---\n# Next Steps\n- Release v1.0 and expand ecosystem\n",
    },
];

pub fn templates_for_app(app_name: &str) -> &'static [DocumentTemplate] {
    match app_name {
        "letters" | "org.tunaos.letters" => LETTERS_TEMPLATES,
        "tables" | "org.tunaos.tables" => TABLES_TEMPLATES,
        "decks" | "org.tunaos.decks" => DECKS_TEMPLATES,
        _ => &[],
    }
}

pub fn find_template_by_id(id: &str) -> Option<&'static DocumentTemplate> {
    LETTERS_TEMPLATES
        .iter()
        .chain(TABLES_TEMPLATES.iter())
        .chain(DECKS_TEMPLATES.iter())
        .find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_templates_available() {
        assert_eq!(templates_for_app("letters").len(), 4);
        assert_eq!(templates_for_app("tables").len(), 3);
        assert_eq!(templates_for_app("decks").len(), 2);
    }

    #[test]
    fn test_find_template() {
        let t = find_template_by_id("letters-formal-letter").expect("template found");
        assert_eq!(t.name, "Formal Letter");
        assert!(t.content.contains("Dear Recipient"));
    }
}
