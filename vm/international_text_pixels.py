"""Bounded owned-fixture pixel proof; no native actions or SDK/public API."""
from collections import Counter

MARKER_ROWS = (0xa7, 0xd2, 0x19, 0xec, 0x56, 0x83, 0x6d, 0xb8)
MARKER_RGB = tuple(bytes(channel for x in range(8) for channel in
                         ((255,)*3 if row & (1 << (7-x)) else (0,)*3)) for row in MARKER_ROWS)


def require(value, label):
    if not value:
        raise ValueError(label)


def read_ppm(data):
    require(isinstance(data, bytes) and len(data) <= 4*1024*1024, 'bounded_owned_ppm')
    position = 0
    def token():
        nonlocal position
        while position < min(len(data), 1024):
            if data[position:position+1].isspace():
                position += 1
            elif data[position:position+1] == b'#':
                end = data.find(b'\n', position)
                require(0 <= end < 1024, 'bounded_owned_ppm_header');position = end+1
            else:
                start = position
                while position < min(len(data), 1024) and not data[position:position+1].isspace():
                    position += 1
                return data[start:position]
        raise ValueError('bounded_owned_ppm_header')
    require(token() == b'P6', 'owned_rgb_ppm_required')
    width, height = token(), token()
    require(width.isdigit() and height.isdigit(), 'owned_ppm_dimensions')
    width, height = int(width), int(height)
    require(0 < width <= 1920 and 0 < height <= 1080 and token() == b'255', 'owned_ppm_dimensions')
    delimiter = 2 if data[position:position+2] == b'\r\n' else 1
    require(data[position:position+1].isspace(), 'owned_ppm_pixel_delimiter')
    pixels = data[position+delimiter:]
    require(len(pixels) == width*height*3, 'exact_owned_ppm_extent')
    return width, height, pixels


def marker(image):
    width, height, pixels = image
    found = []
    offset = pixels.find(MARKER_RGB[0])
    while offset >= 0:
        y, column = divmod(offset, width*3);x, channel = divmod(column, 3)
        if channel == 0 and x+8 <= width and y+8 <= height and all(
                pixels[offset+row*width*3:offset+row*width*3+24] == pattern
                for row, pattern in enumerate(MARKER_RGB)):
            found.append((x, y))
            require(len(found) == 1, 'unique_owned_popup_marker')
        offset = pixels.find(MARKER_RGB[0], offset+1)
    require(len(found) == 1, 'unique_owned_popup_marker')
    return found[0]


def caret_from_popup(image, location, rectangle, field):
    require(isinstance(rectangle, (list, tuple)) and len(rectangle) == 4
            and all(type(value) is int and abs(value) <= 4096 for value in rectangle), 'bounded_popup_local_rectangle')
    dx, dy, width, height = rectangle
    require(0 < width <= 16 and 0 < height <= 256, 'caret_sized_popup_local_rectangle')
    x, y = location[0]+dx, location[1]+dy
    left, top, right, bottom = field
    require(0 <= left < right <= image[0] and 0 <= top < bottom <= image[1]
            and left <= x and x+width <= right and top <= y and y+height <= bottom,
            'reconstructed_caret_in_owned_field')
    return x, y, width, height


def preedit_proof(before, after, before_rectangle, after_rectangle, field):
    require(before[:2] == after[:2], 'same_owned_capture_extent')
    before_marker, after_marker = marker(before), marker(after)
    start = caret_from_popup(before, before_marker, before_rectangle, field)
    end = caret_from_popup(after, after_marker, after_rectangle, field)
    require(end[0] > start[0]+max(8, start[2]*2) and abs(end[1]-start[1]) <= 2,
            'native_preedit_caret_advanced')
    left, right = start[0]+start[2]+3, end[0]-3
    top, bottom = max(start[1], end[1]), min(start[1]+start[3], end[1]+end[3])
    require(right-left >= 8 and bottom-top >= 4, 'noncaret_preedit_ink_band')
    def samples(image):
        width, _, pixels = image
        return {(x,y):tuple(pixels[(y*width+x)*3:(y*width+x+1)*3])
                for y in range(top,bottom) for x in range(left,right)
                if not any(mx <= x < mx+8 and my <= y < my+8 for mx,my in (before_marker,after_marker))}
    old, new = samples(before), samples(after)
    require(bool(old), 'noncaret_preedit_ink_band')
    old_background, old_count = Counter(old.values()).most_common(1)[0]
    new_background, new_count = Counter(new.values()).most_common(1)[0]
    distance = lambda first,second:max(abs(a-b) for a,b in zip(first,second))
    require(old_count*2 >= len(old) and new_count*2 >= len(new)
            and distance(old_background,new_background) <= 2, 'stable_owned_input_background')
    ink = [point for point,value in new.items() if distance(old[point],old_background) <= 2
           and distance(value,new_background) >= 24]
    require(len(ink) >= 12 and len({x for x,_ in ink}) >= 4
            and max(x for x,_ in ink)-min(x for x,_ in ink) >= 6
            and max(y for _,y in ink)-min(y for _,y in ink) >= 3, 'new_noncaret_preedit_ink_required')
    return {'before_marker': before_marker, 'after_marker': after_marker,
            'before_caret': start, 'after_caret': end, 'new_noncaret_ink_pixels': len(ink),
            'scope': 'Owned mapped-popup position plus popup-local hints reconstruct advancing caret; new foreground ink between both excluded caret strips, with stable field background.'}
