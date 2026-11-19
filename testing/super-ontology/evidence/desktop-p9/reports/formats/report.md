# Super Drive formats and failures (T1, real engine)

Result: PASS  | input: synthetic Chromium

| check | result | detail |
|---|---|---|
| every file reached a final state or says why it waits | pass | {"published":28,"unsupported":28,"encrypted":2,"too_large":1} |
| the Markdown, CSV and JSON files are published | pass |  |
| an unknown format is unsupported | pass | unsupported |
| a file over the size limit is too_large | pass | too_large |
| docx-encrypted.docx is encrypted | pass | encrypted |
| pdf-encrypted.pdf is encrypted | pass | encrypted |
| the list shows the engine's state for every file of the folder | pass | {"docx-encrypted.docx":"encrypted","legacy.doc":"unsupported","pdf-encrypted.pdf":"encrypted","too-big.md":"too_large","unknown-format.xyz":"unsupported","zip-bomb-ratio.docx":"unsupported","zip-bomb-size.xlsx":"unsupported","zip-traversal.docx":"unsupported"} vs {"docx-encrypted.docx":"encrypted","legacy.doc":"unsupported","pdf-encrypted.pdf":"encrypted","too-big.md":"too_large","unknown-format.xyz":"unsupported","zip-bomb-ratio.docx":"unsupported","zip-bomb-size.xlsx":"unsupported","zip-traversal.docx":"unsupported"} |
| a Cmd+click adds a second file to the selection | pass | 2 selected |
| the context menu offers the file's actions | pass | Open details, Copy path |
| the upload service accepts a dropped path | pass | {"ok":true,"value":{"batchId":"9663c804-e172-4de8-b39f-363c9dfb963f","items":[{"itemId":"3c594014-b42f-415d-9819-62fb2fe1f97b","name":"dropped.md","relative":"d |
| the uploaded file is stored, read and published | pass | published |

Timings: {"settleMs":4035,"note":"no file of this Drive failed to parse (the failure banner path was not exercised; fixtures missing?)"}

