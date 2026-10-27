// Reproducible, original synthetic fixtures. No Office/PDF application is needed.
import { writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';
import { fileURLToPath } from 'node:url';

function crc32(data) {
  let crc = 0xffffffff;
  for (const byte of data) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}
function zip(parts) {
  const locals = [], directory = [];
  let offset = 0;
  for (const [name, text] of Object.entries(parts)) {
    const data = Buffer.from(text), packed = deflateSync(data), filename = Buffer.from(name);
    // ZIP DEFLATE uses a raw stream, rather than zlib's wrapper.
    const compressed = packed.subarray(2, -4);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50); local.writeUInt16LE(20, 4); local.writeUInt16LE(8, 8);
    local.writeUInt32LE(crc32(data), 14); local.writeUInt32LE(compressed.length, 18);
    local.writeUInt32LE(data.length, 22); local.writeUInt16LE(filename.length, 26);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50); central.writeUInt16LE(20, 4); central.writeUInt16LE(20, 6);
    central.writeUInt16LE(8, 10); central.writeUInt32LE(crc32(data), 16);
    central.writeUInt32LE(compressed.length, 20); central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(filename.length, 28); central.writeUInt32LE(offset, 42);
    locals.push(local, filename, compressed); directory.push(central, filename);
    offset += local.length + filename.length + compressed.length;
  }
  const central = Buffer.concat(directory), end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50); end.writeUInt16LE(Object.keys(parts).length, 8);
  end.writeUInt16LE(Object.keys(parts).length, 10); end.writeUInt32LE(central.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, central, end]);
}
const save = (name, bytes) => writeFileSync(fileURLToPath(new URL(name, import.meta.url)), bytes);
const types = '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/></Types>';
save('sample.docx', zip({
  '[Content_Types].xml': types,
  '_rels/.rels': '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
  'word/document.xml': '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>第一段 &amp; bounded text</w:t></w:r><w:r><w:tab/><w:t>next</w:t></w:r></w:p><w:p><w:r><w:t>Second paragraph</w:t><w:br/><w:t>new line</w:t></w:r></w:p></w:body></w:document>',
}));
save('sample.pptx', zip({
  '[Content_Types].xml': types,
  'ppt/presentation.xml': '<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rSecond"/><p:sldId id="257" r:id="rFirst"/></p:sldIdLst></p:presentation>',
  'ppt/_rels/presentation.xml.rels': '<Relationships><Relationship Id="rFirst" Target="slides/slide1.xml"/><Relationship Id="rSecond" Target="slides/slide2.xml"/><Relationship Id="external" Target="https://example.invalid/never-fetch" TargetMode="External"/></Relationships>',
  'ppt/slides/slide1.xml': '<p:sld xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>Second displayed slide</a:t></a:r></a:p></p:sld>',
  'ppt/slides/slide2.xml': '<p:sld xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>First displayed slide</a:t></a:r></a:p></p:sld>',
}));
save('sample.xlsx', zip({
  '[Content_Types].xml': types,
  'xl/workbook.xml': '<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="数据" sheetId="1" r:id="rData"/><sheet name="Other" sheetId="2" r:id="rOther"/></sheets></workbook>',
  'xl/_rels/workbook.xml.rels': '<Relationships><Relationship Id="rData" Target="worksheets/sheet2.xml"/><Relationship Id="rOther" Target="worksheets/sheet1.xml"/></Relationships>',
  'xl/sharedStrings.xml': '<sst><si><t>Shared 中文</t></si><si><r><t>Rich </t></r><r><t>text</t></r></si></sst>',
  'xl/worksheets/sheet2.xml': '<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1"><v>42</v></c><c r="C1"><f>B1*2</f><v>84</v></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Inline</t></is></c><c r="B2" t="b"><v>1</v></c><c r="C2" t="s"><v>1</v></c></row></sheetData></worksheet>',
  'xl/worksheets/sheet1.xml': '<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Other sheet</t></is></c></row></sheetData></worksheet>',
}));
function pdf(scanned) {
  const form = scanned === 'form';
  const formText = 'BT /F1 12 Tf 10 50 Td (FORM_NEEDLE) Tj ET';
  const stream = form || scanned === 'mixed'
    ? `BT /F1 12 Tf 10 90 Td (Visible heading) Tj ET\nq /${form ? 'Form1' : 'Im1'} Do Q`
    : scanned ? 'q 10 0 0 10 0 0 cm /Im1 Do Q' : 'BT /F1 12 Tf 10 50 Td (PDF page one) Tj ET';
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font << /F1 5 0 R >> /XObject << /${form ? 'Form1' : 'Im1'} 6 0 R >> >> /Contents 4 0 R >>`,
    `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`,
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>',
    form ? `<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /Font << /F1 5 0 R >> >> /Length ${formText.length} >>\nstream\n${formText}\nendstream`
      : '<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\nA\nendstream',
  ];
  let source = '%PDF-1.4\n'; const offsets = [0];
  objects.forEach((object, index) => { offsets.push(Buffer.byteLength(source)); source += `${index + 1} 0 obj\n${object}\nendobj\n`; });
  const xref = Buffer.byteLength(source);
  source += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  source += offsets.slice(1).map(offset => `${String(offset).padStart(10, '0')} 00000 n \n`).join('');
  source += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return source;
}
save('sample.pdf', pdf(false)); save('scanned.pdf', pdf(true));
save('form.pdf', pdf('form')); save('mixed-image.pdf', pdf('mixed'));
function chunk(type, data) {
  const tag = Buffer.from(type), output = Buffer.alloc(12 + data.length);
  output.writeUInt32BE(data.length); tag.copy(output, 4); data.copy(output, 8);
  output.writeUInt32BE(crc32(Buffer.concat([tag, data])), 8 + data.length); return output;
}
const header = Buffer.alloc(13); header.writeUInt32BE(2); header.writeUInt32BE(1, 4); header[8] = 8; header[9] = 2;
save('sample.png', Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]), chunk('IHDR',header), chunk('IDAT',deflateSync(Buffer.from([0,255,0,0,0,0,255]))), chunk('IEND',Buffer.alloc(0))]));
save('sample.txt', '第一行 UTF-8\r\nSecond line\nFinal line');
