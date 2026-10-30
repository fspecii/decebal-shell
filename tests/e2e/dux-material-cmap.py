#!/usr/bin/env python3
"""Verify every native Material alias against the actual font's cmap AND GSUB name.

FONTTOOLS_PATH=/path/to/extracted/fonttools/python python3 tests/e2e/dux-material-cmap.py [OUT]
No installation, network, display, audio or application action is performed.
"""
import hashlib, json, os, re, sys
from pathlib import Path
if os.environ.get('FONTTOOLS_PATH'):
    sys.path.insert(0, os.environ['FONTTOOLS_PATH'])
from fontTools.ttLib import TTFont
repo = Path(__file__).resolve().parents[2]
path = repo / 'themes/dux-lavender/fonts/MaterialSymbolsRounded.ttf'
font = TTFont(path)
cmap = font.getBestCmap()
ligatures = {}
for lookup in font['GSUB'].table.LookupList.Lookup:
    for subtable in lookup.SubTable:
        subtable = getattr(subtable, 'ExtSubTable', subtable)
        for first, entries in getattr(subtable, 'ligatures', {}).items():
            for entry in entries:
                ligatures[(first, *entry.Component)] = entry.LigGlyph
# Independent semantic names. Compatibility aliases are named explicitly; this
# does not assert that every alias is used at every upstream UI position.
names = {
 'apps':'apps','play':'play_arrow','pause':'pause','backward':'skip_previous','forward':'skip_next',
 'bell':'notifications','clipboard':'content_paste','switches':'settings','chart':'space_dashboard',
 'wifi-0':'wifi','wifi-1':'wifi','wifi-2':'wifi','wifi-3':'wifi','wifi-off':'wifi_off','ethernet':'settings_ethernet',
 'speaker-0':'volume_up','speaker-1':'volume_up','speaker-2':'volume_up','speaker-3':'volume_up','speaker-muted':'volume_off',
 'bluetooth':'bluetooth','bluetooth-connected':'bluetooth','bluetooth-off':'bluetooth_disabled','power':'power_settings_new',
 'layout':'view_quilt','sun':'brightness_high','sun-small':'brightness_high','weather-clear':'clear_day',
 'moon':'dark_mode','dark-mode':'dark_mode','weather-moon':'clear_night','night-shift':'nightlight',
 'contrast':'contrast','do_not_disturb_on':'do_not_disturb_on','cup':'local_cafe','screenshot':'screenshot_monitor',
 'weather-partly':'partly_cloudy_day','weather-cloud':'cloud','weather-fog':'foggy','weather-storm':'thunderstorm',
 'weather-snow':'cloudy_snowing','weather-rain':'rainy','gear':'settings','person':'person','close':'close',
 'dashboard':'dashboard','music_note':'music_note','wb_sunny':'wb_sunny','settings':'settings',
 'device_thermostat':'device_thermostat','memory':'memory','developer_board':'developer_board','cloud':'cloud',
 'sunny':'sunny','battery':'battery_full','record':'screen_record','gamepad':'sports_esports','eye':'visibility',
 'magnifyingglass':'search','lock':'lock',
}
body = (repo/'src/menubar/symbols.rs').read_text().split('fn material_codepoint',1)[1].split('type SymbolKey',1)[0]
rows=[]
for aliases, value in re.findall(r'((?:"[^"]+"\s*\|\s*)*"[^"]+")\s*=>\s*(0x[0-9a-f]+)', body):
    cp=int(value,16)
    for alias in re.findall(r'"([^"]+)"',aliases):
        expected=names.pop(alias)
        glyph=ligatures.get(tuple(cmap[ord(c)] for c in expected))
        assert glyph is not None, (alias,expected,'upstream ligature unavailable')
        assert cp in cmap and cmap[cp]==glyph,(alias,value,cmap.get(cp),expected,glyph)
        rows.append({'alias':alias,'codepoint':value,'source_name':expected,'glyph':glyph})
assert not names, ('mapping aliases missing from implementation', sorted(names))
assert len(rows)>=50
result={'font':str(path),'font_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
        'upem':font['head'].unitsPerEm,'ascender':font['hhea'].ascent,'descender':font['hhea'].descent,
        'verified_aliases':len(rows),'rows':rows}
if len(sys.argv)>1:
    Path(sys.argv[1]).write_text(json.dumps(result,indent=2)+'\n')
print('PASS',len(rows),'actual cmap + GSUB semantic aliases',result['font_sha256'])
