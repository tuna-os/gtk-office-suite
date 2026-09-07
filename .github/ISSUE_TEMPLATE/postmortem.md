name: Postmortem Template
description: Standardized postmortem template for operational incidents
title: '[postmortem] '
labels: ['postmortem', 'operations']
body:
  - type: markdown
    attributes:
      value: |
        Complete this postmortem template following resolution of a high-severity operational incident.
  - type: textarea
    id: overview
    attributes:
      label: Incident Overview & Timeline
      description: Summary of the incident and chronological timeline of events.
    validations:
      required: true
  - type: textarea
    id: root_cause
    attributes:
      label: Technical Root Cause
      description: Detailed analysis of the underlying root cause.
    validations:
      required: true
  - type: textarea
    id: preventative_actions
    attributes:
      label: Action Items & Follow-ups
      description: Corrective measures and preventive safeguards to implement.
    validations:
      required: true
