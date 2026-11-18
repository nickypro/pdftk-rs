#!/usr/bin/env python3
# Structural edge cases: direct annots, indirect /AP, inherited resources, contents variants, rotations.
import sys
objs = {}
def add(n, s): objs[n] = s
def ap(n, w, h, body):
    add(n, b"<< /Type /XObject /BBox [0 0 %g %g] /Length %d >>\nstream\n" % (w, h, len(body)) + body + b"\nendstream")
add(1, b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields 60 0 R /NeedAppearances true /SigFlags 1 /DA (/Helv 11 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>")
add(2, b"<< /Type /Pages /Kids [3 0 R 10 0 R 11 0 R 12 0 R] /Count 4 /Resources << /Font << /Helv 5 0 R >> /XObject << /Xi0 30 0 R /Xi2 30 0 R >> >> /MediaBox [0 0 400 300] >>")
add(5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")
ap(30, 10, 10, b"0 1 0 rg 0 0 10 10 re f")
ap(31, 12, 12, b"1 0 0 rg 0 0 12 12 re f")
ap(32, 12, 12, b"0 0 1 rg 0 0 12 12 re f")
ap(33, 100, 20, b"0.5 g 0 0 100 20 re f")
c = b"BT /Helv 12 Tf 20 280 Td (page) Tj ET /Xi0 Do\n"
add(40, b"<< /Length %d >>\nstream\n" % len(c) + c + b"endstream")
add(41, b"<< /Length %d >>\nstream\n" % len(c) + c + b"endstream")
add(42, b"[40 0 R 41 0 R]")
# page 1: direct widget annot, link annot, indirect AP dict, contents = single stream
add(50, b"<< /N 33 0 R >>")
add(3, b"<< /Type /Page /Parent 2 0 R /Contents 40 0 R /Annots [ << /Type /Annot /Subtype /Widget /FT /Tx /T (direct) /F 4 /Rect [20 200 120 220] >> 70 0 R << /Type /Annot /Subtype /Link /Rect [0 0 10 10] /Border [0 0 0] >> 71 0 R 72 0 R 73 0 R 74 0 R 75 0 R ] >>")
add(70, b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (indirect_ap) /F 4 /Rect [20 170 120 190] /AP 50 0 R /V (old) >>")
# radio group: parent no V, kid1 no V, kid2 with V
add(71, b"<< /Type /Annot /Subtype /Widget /Parent 80 0 R /F 4 /Rect [20 140 32 152] /AS /Off /AP << /N << /Off 31 0 R /x 32 0 R >> >> >>")
add(72, b"<< /Type /Annot /Subtype /Widget /Parent 80 0 R /F 4 /Rect [40 140 52 152] /AS /y /V /y /AP << /N << /Off 31 0 R /y 32 0 R >> >> >>")
add(80, b"<< /T (grp) /FT /Btn /Ff 49152 /Kids [71 0 R 72 0 R] >>")
# checkbox with Opt
add(73, b"<< /Type /Annot /Subtype /Widget /FT /Btn /T (chk_opt) /Opt [(export one)] /F 4 /Rect [60 140 72 152] /AS /Off /AP << /N << /Off 31 0 R /0 32 0 R >> >> >>")
# two widgets sharing an AP dict object
add(51, b"<< /N 33 0 R >>")
add(74, b"<< /Type /Annot /Subtype /Widget /Parent 81 0 R /F 4 /Rect [20 110 120 130] /AP 51 0 R >>")
add(75, b"<< /Type /Annot /Subtype /Widget /Parent 81 0 R /F 4 /Rect [140 110 240 130] /AP 51 0 R >>")
add(81, b"<< /T (shared) /FT /Tx /Kids [74 0 R 75 0 R] /V (sharedold) >>")
# page 2: rotate 180, contents indirect array
add(10, b"<< /Type /Page /Parent 2 0 R /Rotate 180 /Contents 42 0 R /Annots 61 0 R >>")
add(61, b"[76 0 R]")
add(76, b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (rot180) /F 4 /Rect [20 200 120 220] /V (r180) /AP << /N 33 0 R >> >>")
# page 3: rotate 270, no contents, own mediabox, own resources
add(11, b"<< /Type /Page /Parent 2 0 R /Rotate 270 /MediaBox [10 20 310 520] /Resources << /Font << /Helv 5 0 R >> >> /Annots [77 0 R 78 0 R] >>")
add(77, b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (rot270) /F 4 /Rect [120 220 20 200] /AP << /N 33 0 R >> >>")
add(78, b"<< /Type /Annot /Subtype /Widget /FT /Btn /T (chk270) /F 4 /Rect [20 100 32 112] /V /On /AS /On /AP << /N << /Off 31 0 R /On 32 0 R >> >> >>")
# page 4: contents direct array, field in Fields but widget not in Annots, sig widget only in Fields
add(12, b"<< /Type /Page /Parent 2 0 R /Contents [40 0 R] /Annots [79 0 R] >>")
add(79, b"<< /Type /Annot /Subtype /Widget /FT /Ch /T (lst) /Opt [(a) (b) (c)] /F 4 /Rect [20 200 120 260] >>")
add(82, b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (orphan) /F 4 /Rect [20 20 120 40] /V (orphanv) >>")
add(83, b"<< /Type /Annot /Subtype /Widget /FT /Sig /T (sigonly) /F 132 /Rect [0 0 0 0] >>")
add(60, b"[70 0 R 80 0 R 73 0 R 81 0 R 76 0 R 77 0 R 78 0 R 79 0 R 82 0 R 83 0 R]")
out = bytearray(b"%PDF-1.5\n%\xe2\xe3\xcf\xd3\n")
offs = {}
for i in sorted(objs):
    offs[i] = len(out)
    out += b"%d 0 obj\n" % i + objs[i] + b"\nendobj\n"
mx = max(objs) + 1
xr = len(out)
out += b"xref\n0 %d\n" % mx
for i in range(mx):
    if i in offs: out += b"%010d 00000 n \n" % offs[i]
    else: out += b"0000000000 65535 f \n"
out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (mx, xr)
open(sys.argv[1], "wb").write(out)
