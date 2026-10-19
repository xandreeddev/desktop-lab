"""Positive/negative owned-popup proof fixtures; no native screenshots or VM."""
import copy
from pathlib import Path
import re
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'vm'))
import international_text_pixels as pixels


class NativePreeditPixelProof(unittest.TestCase):
    field = (4, 4, 116, 58)
    rectangle = (0, -18, 1, 18)

    def canvas(self, marker_x=20, ink=False, caret=True):
        width, height = 120, 70
        data = bytearray((240, 240, 240)*(width*height))
        def dot(x,y,color):
            data[(y*width+x)*3:(y*width+x+1)*3] = bytes(color)
        for row, pattern in enumerate(pixels.MARKER_RGB):
            offset = ((30+row)*width+marker_x)*3;data[offset:offset+24] = pattern
        if caret:
            for y in range(12,30):dot(marker_x,y,(0,0,0))
        if ink:
            for left in (26,39,52):
                for y in range(15,23):
                    for x in range(left,left+5):dot(x,y,(32,32,32))
        return width,height,bytes(data)

    def test_opaque_marker_matches_the_independently_compiled_rust_peer(self):
        source=(ROOT/'lucent/examples/international-text/src/bin/input-method-peer.rs').read_text()
        rows=re.search(r'const MARKER_ROWS: \[u8; 8\] = \[([^\]]+)\]',source).group(1)
        self.assertEqual(tuple(int(value.strip(),0) for value in rows.split(',')),pixels.MARKER_ROWS)

    def test_new_ink_between_excluded_carets_and_reconstructed_motion_passes(self):
        result=pixels.preedit_proof(self.canvas(),self.canvas(62,True),self.rectangle,self.rectangle,self.field)
        self.assertEqual(result['before_caret'],(20,12,1,18))
        self.assertEqual(result['after_caret'],(62,12,1,18))
        self.assertEqual(result['new_noncaret_ink_pixels'],120)

    def test_caret_flash_and_even_moving_marker_without_preedit_ink_fail(self):
        with self.assertRaisesRegex(ValueError,'native_preedit_caret_advanced'):
            pixels.preedit_proof(self.canvas(),self.canvas(caret=False),self.rectangle,self.rectangle,self.field)
        with self.assertRaisesRegex(ValueError,'new_noncaret_preedit_ink_required'):
            pixels.preedit_proof(self.canvas(),self.canvas(62,caret=False),self.rectangle,self.rectangle,self.field)

    def test_reused_image_with_a_changed_local_hint_cannot_fabricate_ink(self):
        same=self.canvas()
        with self.assertRaisesRegex(ValueError,'new_noncaret_preedit_ink_required'):
            pixels.preedit_proof(same,same,self.rectangle,(42,-18,1,18),self.field)

    def test_new_ink_without_actual_caret_motion_fails(self):
        with self.assertRaisesRegex(ValueError,'native_preedit_caret_advanced'):
            pixels.preedit_proof(self.canvas(),self.canvas(20,True),self.rectangle,self.rectangle,self.field)

    def test_wrong_region_or_only_horizontal_stroke_is_not_preedit_ink(self):
        for horizontal in (False,True):
            width,height,raw=self.canvas(62);data=bytearray(raw)
            for y in (range(16,17) if horizontal else range(15,23)):
                for x in (range(26,57) if horizontal else range(82,95)):
                    data[(y*width+x)*3:(y*width+x+1)*3]=b'\x20'*3
            with self.subTest(horizontal=horizontal),self.assertRaisesRegex(ValueError,'new_noncaret_preedit_ink_required'):
                pixels.preedit_proof(self.canvas(),(width,height,bytes(data)),self.rectangle,self.rectangle,self.field)

    def test_missing_or_nonunique_marker_fails_before_interpreting_hints(self):
        width,height,raw=self.canvas(62,True)
        for duplicate in (False,True):
            data=bytearray(raw)
            for row,pattern in enumerate(pixels.MARKER_RGB):
                offset=((30+row)*width+62)*3
                if not duplicate:data[offset:offset+24]=b'\xf0'*24
                else:
                    offset=((40+row)*width+94)*3;data[offset:offset+24]=pattern
            with self.subTest(duplicate=duplicate),self.assertRaisesRegex(ValueError,'unique_owned_popup_marker'):
                pixels.preedit_proof(self.canvas(),(width,height,bytes(data)),self.rectangle,self.rectangle,self.field)

    def test_background_change_is_not_accepted_as_glyph_coverage(self):
        width,height,raw=self.canvas(62,True);data=bytearray(raw)
        for index in range(0,len(data),3):
            if data[index:index+3]==b'\xf0'*3:data[index:index+3]=b'\xd0'*3
        with self.assertRaisesRegex(ValueError,'stable_owned_input_background'):
            pixels.preedit_proof(self.canvas(),(width,height,bytes(data)),self.rectangle,self.rectangle,self.field)

    def test_ppm_parser_keeps_pixel_whitespace_and_refuses_extent_overflow(self):
        image=pixels.read_ppm(b'P6\n# owned fixture\n1 1\n255\r\n\n\r ')
        self.assertEqual(image,(1,1,b'\n\r '))
        for data in (b'P6\n1 1\n255\n\x00',b'P6\n2000 1\n255\n',b'P6\n1 1\n65535\n',b'P6\n#'+b'x'*1024):
            with self.subTest(data_length=len(data)),self.assertRaises(ValueError):pixels.read_ppm(data)


if __name__=='__main__':unittest.main()
