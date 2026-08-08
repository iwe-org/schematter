---
type: Metric
title: Annual Recognized Revenue
description: Recognized revenue for a fiscal year, summed over booked orders.
resource: https://console.example.com/metrics/annual-revenue
tags: [finance, revenue]
generated: { by: reference_agent/gemini-2.5-pro, at: 2026-05-28T14:30:00Z }
verified: { by: human:author, at: 2026-06-25T09:00:00Z }
stale_after: 2026-12-31
sources:
  - id: revenue-policy
    resource: https://docs.example.com/finance/revenue-recognition
    title: Revenue recognition policy
    author: team:finance
    last_modified: 2026-05-30
---

# Definition

Recognized revenue sums booked orders over the fiscal year, excluding orders
cancelled before fulfilment.[^revenue-policy]

[^revenue-policy]: Revenue recognition policy

# Examples

``` sql
SELECT SUM(amount) FROM orders WHERE fiscal_year = 2026 AND status = 'booked'
```
