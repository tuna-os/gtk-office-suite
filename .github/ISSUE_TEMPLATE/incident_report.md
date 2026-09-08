name: Incident Report
description: Report an operational incident, component failure, or release outage
title: '[incident] '
labels: ['incident', 'operations']
body:
  - type: markdown
    attributes:
      value: |
        Please provide details on the operational incident observed in gtk-office-suite components.
  - type: textarea
    id: summary
    attributes:
      label: Incident Summary
      description: Concise summary of the operational failure or crash.
    validations:
      required: true
  - type: textarea
    id: environment
    attributes:
      label: Environment & Component Info
      description: Operating environment, Flatpak version, GTK version, or affected crate.
    validations:
      required: true
  - type: textarea
    id: logs
    attributes:
      label: Relevant Log Snippets (`journalctl`)
      description: Attach relevant error outputs or diagnostic logs.
    validations:
      required: false
  - type: textarea
    id: impact
    attributes:
      label: User & Operational Impact
      description: Scope and severity of the impact on end users or builds.
    validations:
      required: true
