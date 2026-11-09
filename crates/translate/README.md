# pdfcraft-translate

Translate the text of a PDF with Google Cloud Translation or Gemini and get back a review page a person can check. The headless tool is `text_translate_review` in `pdfcraft-automation` (`pdfcraft-cli tools` lists it).

## What it does

1. Takes the text of the chosen pages and splits it into segments that fit one request.
2. Translates each segment (the target defaults to Sinhala, `si`; any language the service supports works as the source, or let it detect the language).
3. Translates each result back into the original language, with the same service or a second one (`back_engine`).
4. Writes one HTML page, with no scripts, listing for every segment the original, the translation, the translation translated back and any automatic notes.

The notes are advice, not a verdict. They flag:

- numbers that appear on one side only (numerals of other writing systems count as the same number);
- web and e-mail addresses that changed;
- text returned unchanged, or with no letters in the target script (checked for Sinhala);
- a back-translation that matches the original poorly (a measure of shared letter pairs, which works for scripts written without spaces).

**A row without a note is not a verified row.** Machine translation cannot be proven correct, a back-translation by the same service tends to repeat that service's mistakes, and the similarity threshold (`min_similarity`, default 0.45) is a starting value that has not been calibrated. Have a reader of both languages check anything that matters.

## Services

| `engine` | What it is | Key variable | Cost |
|---|---|---|---|
| `google` (default) | Google Cloud Translation Basic, a translation engine | `PDFCRAFT_GOOGLE_TRANSLATE_API_KEY` | Billed to the key's project. Google lists the first 500,000 characters each month as free, then a price per million characters; check [the pricing page](https://cloud.google.com/translation/pricing). A billing account is required. |
| `gemini` | A Gemini language model through the Google AI Studio API | `PDFCRAFT_GEMINI_API_KEY`, optionally `PDFCRAFT_GEMINI_MODEL` (default `gemini-3.5-flash-lite`) | A free tier exists without a card, with rate limits. Check Google's current limits. |

A language model is not a translation engine. It can rephrase, leave a sentence out, or follow an instruction hidden in the document, and its Sinhala has not been measured here. The prompt tells it to translate literally and to treat the text as data, and the answer must be a list of the right length, but the review is the only protection. A good setup is `engine: google` with `back_engine: gemini`: the check is then made by a different system than the translation.

On Gemini's free tier Google may use the text you send to improve its products (see Google's Gemini API terms). Do not send private documents that way.

Free tiers allow only a few requests a minute. A run that hits the limit stops with a message saying so; it does not retry.

## Privacy and cost

- The page text is sent to the service you chose. The tool refuses to run unless `confirm_send` is `true`.
- A run is refused when it would send more than `max_chars` characters (default 100,000, back-translation counted). Check the current pricing and limits before raising it.
- Keys are read from environment variables. A key is never an argument, is not stored, and does not appear in errors or logs. The Gemini key is sent in a header, the Google Translate key in the URL query as that API requires. Restrict each key to its API in the Google console.
- Nothing runs by default and no other part of PdfCraft uses the network for this.

## Limits

- Pages with no extractable text (scans) need OCR first (`ocr_recognize`).
- Text is joined line by line into sentences, so tables and multi-column pages may translate in a confusing order. The original column in the review shows exactly what was sent.
- The review page is a table, not a translated PDF. Writing Sinhala into a PDF needs a Sinhala font and complex-script shaping, which this crate does not do.

## Browser build

The browser (wasm32) build has no sockets, so it carries no HTTP client: there, every request fails with a clear "not available in this build" error. Use the desktop app or `pdfcraft-cli`.
