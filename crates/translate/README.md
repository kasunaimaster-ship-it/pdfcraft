# pdfcraft-translate

Translate the text of a PDF with Google Cloud Translation and get back a review page a person can check. The headless tool is `text_translate_review` in `pdfcraft-automation` (`pdfcraft-cli tools` lists it).

## What it does

1. Takes the text of the chosen pages and splits it into segments that fit one request.
2. Translates each segment (the target defaults to Sinhala, `si`; any language Google supports works as the source, or let Google detect it).
3. Translates each result back into the original language.
4. Writes one HTML page, with no scripts, listing for every segment the original, the translation, the translation translated back and any automatic notes.

The notes are advice, not a verdict. They flag:

- numbers that appear on one side only (numerals of other writing systems count as the same number);
- web and e-mail addresses that changed;
- text returned unchanged, or with no letters in the target script (checked for Sinhala);
- a back-translation that matches the original poorly (a measure of shared letter pairs, which works for scripts written without spaces).

**A row without a note is not a verified row.** Machine translation cannot be proven correct, the back-translation uses the same service as the translation, and the similarity threshold (`min_similarity`, default 0.45) is a starting value that has not been calibrated. Have a reader of both languages check anything that matters.

## Privacy and cost

- The page text is sent to Google. The tool refuses to run unless `confirm_send` is `true`.
- Google bills the project that owns the API key. A run is refused when it would send more than `max_chars` characters (default 100,000, back-translation counted). Check Google's current pricing before raising it.
- The key is read from the `PDFCRAFT_GOOGLE_TRANSLATE_API_KEY` environment variable. It is never an argument, is not stored, and does not appear in errors or logs. Restrict the key to the Cloud Translation API in the Google Cloud console.
- Nothing runs by default and no other part of PdfCraft uses the network for this.

## Limits

- Pages with no extractable text (scans) need OCR first (`ocr_recognize`).
- Text is joined line by line into sentences, so tables and multi-column pages may translate in a confusing order. The original column in the review shows exactly what was sent.
- The review page is a table, not a translated PDF. Writing Sinhala into a PDF needs a Sinhala font and complex-script shaping, which this crate does not do.
