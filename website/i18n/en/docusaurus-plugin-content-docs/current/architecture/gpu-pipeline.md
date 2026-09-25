---
title: GPU pipeline
---

# GPU pipeline

The renderer prepares paths, brushes, layers, text, and images from the same scene model for each native API. A path scan and prefix pass calculate work, coarse binning assigns draws to 16×16 tiles, and fine rasterization resolves coverage and blending. Filter passes use intermediate surfaces as needed. Retained frames reuse unchanged scene records and output history. Image readback is explicit through `NativeImageSubmission::readback`; host targets use submission receipts to coordinate presentation.
