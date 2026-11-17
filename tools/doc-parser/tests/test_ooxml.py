import zipfile

from doc_parser.ooxml import docx_facts

NS = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'


def make(tmp_path, body, styles=""):
    p = tmp_path / "a.docx"
    with zipfile.ZipFile(p, "w") as z:
        z.writestr("word/document.xml", f'<?xml version="1.0"?><w:document {NS}><w:body>{body}</w:body></w:document>')
        if styles:
            z.writestr("word/styles.xml", f'<?xml version="1.0"?><w:styles {NS}>{styles}</w:styles>')
    return p


def para(text, style=None):
    ppr = f'<w:pPr><w:pStyle w:val="{style}"/></w:pPr>' if style else ""
    return f"<w:p>{ppr}<w:r><w:t>{text}</w:t></w:r></w:p>"


def test_body_paragraph_ordinals_skip_table_paragraphs(tmp_path):
    table = "<w:tbl><w:tr><w:tc>%s</w:tc><w:tc>%s</w:tc></w:tr></w:tbl>" % (para("in-cell 1"), para("in-cell 2"))
    body = para("Title", "H1") + para("first") + table + "<w:p/>" + para("after table")
    styles = '<w:style w:styleId="H1"><w:name w:val="heading 1"/></w:style>'
    f = docx_facts(make(tmp_path, body, styles))
    assert [p["text"] for p in f["paragraphs"]] == ["Title", "first", "", "after table"]
    assert f["paragraphs"][0]["level"] == 1 and f["paragraphs"][0]["style"] == "heading 1"
    assert f["paragraphs"][1]["level"] is None
    assert f["tables"] == [{"before": 2, "rows": 1, "cols": 2}]


def test_tabs_breaks_and_deleted_text(tmp_path):
    body = (
        f'<w:p><w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t></w:r>'
        f'<w:del><w:r><w:t>gone</w:t></w:r></w:del></w:p>'
    )
    f = docx_facts(make(tmp_path, body))
    assert f["paragraphs"][0]["text"] == "a\tb\nc"


def test_grid_span_counts_columns_and_outline_level(tmp_path):
    tc = '<w:tc><w:tcPr><w:gridSpan w:val="3"/></w:tcPr><w:p/></w:tc>'
    body = f"<w:tbl><w:tr>{tc}</w:tr><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>"
    body += '<w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>x</w:t></w:r></w:p>'
    f = docx_facts(make(tmp_path, body))
    assert f["tables"][0] == {"before": 0, "rows": 2, "cols": 3}
    assert f["paragraphs"][0]["level"] == 2
