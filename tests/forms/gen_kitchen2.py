#!/usr/bin/env python3
# Generates a synthetic AcroForm test PDF exercising many field kinds.
import sys
objs = {}
def add(n, s): objs[n] = s
def ap(n, w, h, body=b"/Tx BMC\nEMC\n"):
    add(n, b"<< /Type /XObject /Subtype /Form /BBox [0 0 %g %g] /Resources << /Font << /Helv 5 0 R /ZaDb 7 0 R >> >> /Length %d >>\nstream\n" % (w, h, len(body)) + body + b"\nendstream")
add(1, b"<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>")
add(2, b"<< /Type /Pages /Kids [3 0 R 40 0 R] /Count 2 >>")
add(5, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")
add(6, b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding << /Type /Encoding /BaseEncoding /WinAnsiEncoding /Differences [ 128 /Euro ] >> >>")
add(7, b"<< /Type /Font /Subtype /Type1 /BaseFont /ZapfDingbats >>")
add(8, b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>")
fields = []
tu = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CMapName /Adobe-Identity-UCS def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange 3 beginbfchar <0003> <0020> <0010> <4E2D> <0011> <6587> endbfchar 2 beginbfrange <0024> <003D> <0041> <0044> <0046> [<0061> <0062> <0063>] endbfrange endcmap end end"
add(20, b"<< /Length %d >>\nstream\n" % len(tu) + tu + b"\nendstream")
add(21, b"<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+MyCID /Encoding /Identity-H /DescendantFonts [22 0 R] /ToUnicode 20 0 R >>")
add(22, b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+MyCID /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /DW 700 /W [3 [250] 16 17 1000 36 [600 610 620] 68 70 444] /FontDescriptor 23 0 R >>")
add(23, b"<< /Type /FontDescriptor /FontName /ABCDEF+MyCID /Flags 4 /FontBBox [-100 -250 1100 950] /ItalicAngle 0 /Ascent 880 /Descent -120 /CapHeight 700 /StemV 80 >>")
add(24, b"<< /Type /Font /Subtype /TrueType /BaseFont /My#20Mac /Encoding /MacRomanEncoding /FirstChar 32 /LastChar 255 /Widths [" + b" ".join(b"%d" % (300 + (i * 7) % 400) for i in range(224)) + b"] /FontDescriptor 25 0 R >>")
add(25, b"<< /Type /FontDescriptor /FontName /My#20Mac /Flags 32 /FontBBox [900 800 -200 -300] /ItalicAngle 0 /Ascent 750 /Descent -250 /CapHeight 700 /StemV 80 >>")
add(26, b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Bold >>")
add(27, b"<< /Type /Font /Subtype /Type1 /BaseFont /NoWidthsFont /Encoding << /Differences [65 /Euro /eacute 200 /A /B] >> >>")
add(28, b"<< /Type /Font /Subtype /Type1 /BaseFont /Symbol >>")
annots1 = []
annots2 = []
n = [50]
def nid():
    n[0] += 1
    return n[0]
def field(body, page=1, top=True):
    i = nid()
    add(i, b"<< " + body + (b" /P %d 0 R" % (3 if page == 1 else 40)) + b" >>")
    if top: fields.append(i)
    return i
def widget(body, page=1, top=True):
    i = field(b"/Type /Annot /Subtype /Widget " + body, page, top)
    (annots1 if page == 1 else annots2).append(i)
    return i
y = [760]
def rect(w=200, h=20, x=50):
    y[0] -= h + 8
    return b"/Rect [%g %g %g %g]" % (x, y[0], x + w, y[0] + h)
# plain text fields
widget(b"/FT /Tx /T (text_left) /DA (/Helv 12 Tf 0 g) /F 4 " + rect())
widget(b"/FT /Tx /T (text_center) /Q 1 /DA (/Helv 12 Tf 0 0 1 rg) /F 4 " + rect())
widget(b"/FT /Tx /T (text_right) /Q 2 /DA (/Helv 9 Tf 0.5 g) /F 4 " + rect())
widget(b"/FT /Tx /T (text_auto) /DA (/Helv 0 Tf 0 g) /F 4 " + rect())
widget(b"/FT /Tx /T (text_auto_long) /DA (/Helv 0 Tf 0 g) /F 4 " + rect(w=120))
widget(b"/FT /Tx /T (text_times) /DA (/TiRo 11 Tf 1 0 0 rg) /F 4 " + rect())
widget(b"/FT /Tx /T (text_cour) /DA (/Cour 10 Tf 0 0 0 1 k) /F 4 " + rect())
widget(b"/FT /Tx /T (text_nodr) /DA (/HeBo 10 Tf) /F 4 " + rect())
widget(b"/FT /Tx /T (text_noda) /F 4 " + rect())
widget(b"/FT /Tx /T (text_maxlen) /MaxLen 5 /DA (/Helv 12 Tf 0 g) /F 4 " + rect())
widget(b"/FT /Tx /T (text_comb) /Ff 16777216 /MaxLen 8 /DA (/Helv 12 Tf 0 g) /F 4 /MK << /BC [0 0 0] >> " + rect())
widget(b"/FT /Tx /T (text_comb_center) /Q 1 /Ff 16777216 /MaxLen 8 /DA (/Helv 0 Tf 0 g) /F 4 /MK << /BC [0 0 0] /BG [1 1 0.8] >> " + rect())
widget(b"/FT /Tx /T (text_password) /Ff 8192 /DA (/Helv 12 Tf 0 g) /F 4 " + rect())
widget(b"/FT /Tx /T (text_multi) /Ff 4096 /DA (/Helv 10 Tf 0 g) /F 4 " + rect(h=60))
widget(b"/FT /Tx /T (text_multi_auto) /Ff 4096 /DA (/Helv 0 Tf 0 g) /F 4 /MK << /BC [1 0 0] /BG [0.9] >> " + rect(h=50))
widget(b"/FT /Tx /T (text_multi_center) /Q 1 /Ff 4096 /DA (/Helv 10 Tf 0 g 14 TL) /F 4 " + rect(h=60))
widget(b"/FT /Tx /T (text_border_bevel) /DA (/Helv 12 Tf 0 g) /F 4 /MK << /BC [0 0 1] /BG [0.8 0.9 1] >> /BS << /W 2 /S /B >> " + rect(h=24))
widget(b"/FT /Tx /T (text_border_inset) /DA (/Helv 12 Tf 0 g) /F 4 /MK << /BC [0 0 1] >> /BS << /W 2 /S /I >> " + rect(h=24))
widget(b"/FT /Tx /T (text_border_under) /DA (/Helv 12 Tf 0 g) /F 4 /MK << /BC [0 0 1] >> /BS << /W 1 /S /U >> " + rect())
widget(b"/FT /Tx /T (text_border_dash) /DA (/Helv 12 Tf 0 g) /F 4 /MK << /BC [0 1 0 0] >> /BS << /W 1 /S /D >> " + rect())
widget(b"/FT /Tx /T (text_border_arr) /DA (/Helv 12 Tf 0 g) /F 4 /MK << /BC [0.3] >> /Border [0 0 3] " + rect(h=24))
widget(b"/FT /Tx /T (text_rot90) /DA (/Helv 12 Tf 0 g) /F 4 /MK << /R 90 /BC [0 0 0] >> " + b"/Rect [300 600 320 740]")
widget(b"/FT /Tx /T (text_hidden) /DA (/Helv 12 Tf 0 g) /F 6 " + b"/Rect [340 700 540 720]")
widget(b"/FT /Tx /T (text_noprint) /DA (/Helv 12 Tf 0 g) /F 0 " + b"/Rect [340 670 540 690]")
widget(b"/FT /Tx /T (text_noview) /DA (/Helv 12 Tf 0 g) /F 36 " + b"/Rect [340 640 540 660]")
widget(b"/FT /Tx /T (text_unicode) /DA (/Helv 12 Tf 0 g) /F 4 /TU (Alt \\351 name) " + b"/Rect [340 610 540 630]")
widget(b"/FT /Tx /T (text_readonly) /Ff 1 /DA (/Helv 12 Tf 0 g) /F 4 /V (ro) /DV (ro) " + b"/Rect [340 580 540 600]")
widget(b"/FT /Tx /T (f_cid) /DA (/FCID 11 Tf 0 g) /F 4 /Rect [50 80 250 100]")
widget(b"/FT /Tx /T (f_cid_auto) /Ff 4096 /DA (/FCID 0 Tf 0 g) /F 4 /Rect [50 40 250 75]")
widget(b"/FT /Tx /T (f_mac) /Q 2 /DA (/FMac 11 Tf 0 g) /F 4 /Rect [260 80 330 100]")
widget(b"/FT /Tx /T (f_mac_auto) /DA (/FMac 0 Tf 0 g) /F 4 /Rect [260 40 330 75]")
widget(b"/FT /Tx /T (f_std) /Q 1 /DA (/FStd 11 Tf 0 g) /F 4 /Rect [340 110 540 130]")
widget(b"/FT /Tx /T (f_diff) /DA (/FDiff 11 Tf 0 g) /F 4 /Rect [340 80 540 100]")
widget(b"/FT /Tx /T (f_sym) /DA (/FSym 11 Tf 0 g) /F 4 /Rect [340 50 540 70]")
widget(b"/FT /Tx /T (f_dir) /DA (/Dir 11 Tf 0 g) /F 4 /Rect [340 20 540 40]")
widget(b"/FT /Tx /T (f_zadb) /DA (/ZaDb 11 Tf 0 g) /F 4 /Rect [50 10 250 30]")
widget(b"/FT /Tx /T (f_symb_builtin) /DA (/Symb 11 Tf 0 g) /F 4 /Rect [260 10 330 30]")
# hierarchical
k1 = widget(b"/T (child1) /DA (/Helv 12 Tf 0 g) /F 4 /Rect [340 550 540 570] /Parent 30 0 R", top=False)
k2 = widget(b"/T (child2) /Q 2 /F 4 /Rect [340 520 540 540] /Parent 30 0 R", top=False)
add(30, b"<< /T (parent) /FT /Tx /DA (/Cour 9 Tf 0 g) /Kids [%d 0 R %d 0 R] >>" % (k1, k2)); fields.append(30)
# same field, two widgets
w1 = widget(b"/F 4 /Rect [340 490 430 510] /Parent 31 0 R", top=False)
w2 = widget(b"/F 4 /Rect [450 490 540 510] /Parent 31 0 R /Q 1", top=False)
add(31, b"<< /T (twowidgets) /FT /Tx /DA (/Helv 12 Tf 0 g) /Kids [%d 0 R %d 0 R] >>" % (w1, w2)); fields.append(31)
# checkbox
ap(32, 12, 12); ap(33, 12, 12, b"q BT /ZaDb 10 Tf 1 2 Td (4) Tj ET Q\n")
widget(b"/FT /Btn /T (check1) /V /Off /AS /Off /F 4 /Rect [340 460 352 472] /AP << /N << /Off 32 0 R /Yes 33 0 R >> >>")
widget(b"/FT /Btn /T (check2) /V /On /AS /On /F 4 /Rect [360 460 372 472] /AP << /N << /Off 32 0 R /On 33 0 R >> /D << /Off 32 0 R /On 33 0 R >> >>")
# radio
r1 = widget(b"/AS /Off /F 4 /Rect [340 430 352 442] /Parent 34 0 R /AP << /N << /Off 32 0 R /a 33 0 R >> >>", top=False)
r2 = widget(b"/AS /b /F 4 /Rect [360 430 372 442] /Parent 34 0 R /AP << /N << /Off 32 0 R /b 33 0 R >> >>", top=False)
r3 = widget(b"/AS /Off /F 4 /Rect [380 430 392 442] /Parent 34 0 R /AP << /N << /Off 32 0 R /c#20d 33 0 R >> >>", top=False)
add(34, b"<< /T (radio1) /FT /Btn /Ff 49152 /V /b /Kids [%d 0 R %d 0 R %d 0 R] >>" % (r1, r2, r3)); fields.append(34)
# radio with Opt
q1 = widget(b"/AS /Off /F 4 /Rect [340 400 352 412] /Parent 35 0 R /AP << /N << /Off 32 0 R /0 33 0 R >> >>", top=False)
q2 = widget(b"/AS /Off /F 4 /Rect [360 400 372 412] /Parent 35 0 R /AP << /N << /Off 32 0 R /1 33 0 R >> >>", top=False)
add(35, b"<< /T (radio_opt) /FT /Btn /Ff 49152 /Opt [(first) (second \\(2\\))] /Kids [%d 0 R %d 0 R] >>" % (q1, q2)); fields.append(35)
# pushbutton
widget(b"/FT /Btn /T (push) /Ff 65536 /F 4 /Rect [340 370 400 390] /MK << /CA (Go) >>")
# choices
widget(b"/FT /Ch /T (combo) /Ff 131072 /Opt [(red) (green) (blue)] /V (green) /DA (/Helv 10 Tf 0 g) /F 4 /Rect [340 340 540 360]")
widget(b"/FT /Ch /T (combo_pairs) /Ff 131072 /Opt [[(r) (Red colour)] [(g) (Green colour)] [(b) (Blue colour)]] /DA (/Helv 10 Tf 0 g) /F 4 /Rect [340 310 540 330]")
widget(b"/FT /Ch /T (combo_edit) /Ff 393216 /DA (/Helv 10 Tf 0 g) /F 4 /Rect [340 280 540 300]")
widget(b"/FT /Ch /T (list) /Opt [(one) (two) (three) (four) (five) (six) (seven)] /DA (/Helv 10 Tf 0 g) /F 4 /MK << /BC [0 0 0] >> /Rect [340 200 440 270]")
widget(b"/FT /Ch /T (list_multi) /Ff 2097152 /Opt [[(1) (one)] [(2) (two)] [(3) (three)] [(4) (four)]] /V [(1) (3)] /I [0 2] /DA (/Helv 0 Tf 0 g) /F 4 /MK << /BC [0 0 0] >> /Rect [450 200 540 270]")
widget(b"/FT /Sig /T (sig) /F 4 /Rect [340 170 540 190]")
# page 2 (rotated page)
widget(b"/FT /Tx /T (p2.text) /DA (/Helv 12 Tf 0 g) /F 4 /Rect [50 700 250 720]", page=2)
widget(b"/FT /Tx /T (p2_rot) /DA (/Helv 12 Tf 0 g) /F 4 /MK << /R 90 >> /Rect [300 600 320 740]", page=2)
ap(36, 12, 12, b"q 0 0 1 rg 0 0 12 12 re f Q\n")
widget(b"/FT /Btn /T (p2_check) /V /Yes /AS /Yes /F 4 /Rect [50 650 74 674] /AP << /N << /Off 32 0 R /Yes 36 0 R >> >>", page=2)
c1 = b"BT /Helv 14 Tf 50 780 Td (Form test page one) Tj ET\n"
add(9, b"<< /Length %d >>\nstream\n" % len(c1) + c1 + b"endstream")
c2 = b"BT /Helv 14 Tf 50 780 Td (Form test page two) Tj ET\n"
add(10, b"<< /Length %d >>\nstream\n" % len(c2) + c2 + b"endstream")
add(3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 9 0 R /Resources << /Font << /Helv 5 0 R >> >> /Annots [" + b" ".join(b"%d 0 R" % a for a in annots1) + b"] >>")
add(40, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Rotate 90 /Contents 10 0 R /Resources << /Font << /Helv 5 0 R >> >> /Annots [" + b" ".join(b"%d 0 R" % a for a in annots2) + b"] >>")
add(4, b"<< /Fields [" + b" ".join(b"%d 0 R" % f for f in fields) + b"] /DR << /Font << /Helv 5 0 R /TiRo 6 0 R /ZaDb 7 0 R /Cour 8 0 R /FCID 21 0 R /FMac 24 0 R /FStd 26 0 R /FDiff 27 0 R /FSym 28 0 R /Dir << /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Oblique >> >> >> /DA (/Helv 0 Tf 0 g) >>")
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
