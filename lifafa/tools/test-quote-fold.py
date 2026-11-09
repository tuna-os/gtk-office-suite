#!/usr/bin/env python3
# Regression check for the reader's quote folding (SIZE_SCRIPT `quote()` in
# src/ui/message_view.rs): extracts the script from the source, loads it in a
# real WebKitGTK view with one iframe per case, and reports which bodies get a
# ••• button and what stays visible. Then it opens every fold and checks the
# button moved to where the quote starts, so it can be closed again (#326).
# Needs a display and python3-gobject with WebKit 6.0.
#
#   tools/test-quote-fold.py            # prints ok/FAIL per case, exits 1 on any FAIL
import re, json, sys, os, gi
gi.require_version('Gtk','4.0'); gi.require_version('WebKit','6.0')
from gi.repository import Gtk, WebKit, GLib
src=open(os.path.join(os.path.dirname(os.path.abspath(__file__)),'..','src','ui','message_view.rs'),encoding='utf-8').read()
m=re.search(r'const SIZE_SCRIPT: &str = "(.*?)";\n', src, re.S)
raw=m.group(1).replace('\\\n','')
# decode rust string escapes
js=re.sub(r'\\u\{([0-9a-fA-F]+)\}', lambda x: chr(int(x.group(1),16)), raw)
js=js.replace('\\"','"').replace('\\\\','\\')
# cut before DOMContentLoaded handler: keep function defs only
js=js[:js.index('document.addEventListener(\'DOMContentLoaded\'')]
stubs="var follow=null,hold=null;function chase(){}function pin(){}function reportPos(){}function markClipped(){}function selAll(){}function copySel(){}\n"
P='<div class="vireo-plain">{}</div>'
OUTLOOK_HDR=('<div style="border:none;border-top:solid #E1E1E1 1.0pt;padding:3.0pt 0in 0in 0in">'
             '<p><b>From:</b> Ann<br><b>Sent:</b> Monday<br><b>To:</b> Bob<br><b>Subject:</b> Plans</p></div>')
# name: (body, folded, text that must stay visible or None)
cases={
 'top_post':('<p>Reply</p><blockquote>quote</blockquote>',True,'Reply'),
 'interleaved':('<p>Hi John,</p><blockquote>John a ecrit: Hello bob, yada</blockquote><p>I dont understand what you mean...</p>',False,None),
 'top_post_sig_after':('<p>Reply</p><blockquote>q</blockquote><p>-- <br>Bob</p>',True,None),
 'outlook_header':('<p>Reply</p><div id="divRplyFwdMsg">From: x</div><div>original body text here</div>',True,'Reply'),
 'bottom_post':('<blockquote>q</blockquote><p>reply</p>',False,None),
 'gmail':('<div>reply</div><div class="gmail_quote">On x wrote:<blockquote class="gmail_quote">q</blockquote></div><div><br></div>',True,'reply'),
 'two_quotes_then_sigclass':('<p>r</p><blockquote>q</blockquote><br><blockquote>q2</blockquote><div class="moz-signature">-- Bob</div>',True,None),
 'list_footer':('<p>r</p><blockquote>q</blockquote><p>_______________________________________________<br>foo mailing list</p>',True,None),
 'interleaved_multi':('<p>Hi</p><blockquote>q1</blockquote><p>a1</p><blockquote>q2</blockquote><p>a2</p>',False,None),
 'nbsp_after':('<p>r</p><blockquote>q</blockquote><p>&nbsp;</p>',True,None),
 'thunderbird_interleaved':('<div class="moz-cite-prefix">John wrote:</div><blockquote type="cite">q</blockquote><p>answer</p>',False,None),
 'text_node_after':('<p>r</p><blockquote>q</blockquote>plain reply text after',False,None),
 # #326: one wrapper around the whole message used to stop the fold.
 'wrapped_gmail':('<div dir="ltr"><div>reply</div><div class="gmail_quote"><blockquote>q</blockquote></div></div>',True,'reply'),
 'wrapped_with_style':('<style>p{margin:0}</style><div class="WordSection1"><p>reply</p><blockquote>q</blockquote></div>',True,'reply'),
 'outlook_desktop':('<div class="WordSection1"><p>reply</p><div>'+OUTLOOK_HDR+'</div><p>original</p></div>',True,'reply'),
 'receipt_border':('<div style="border-top:1px solid"><b>Total:</b> 5</div><p>more</p>',False,None),
 'original_message':('<p>Thanks</p><p>-----Original Message-----<br>From: x</p><p>old</p>',True,'Thanks'),
 'apple_attribution':('<div>Sounds good</div><br><div>On 1 Oct 2026, at 09:00, Ann &lt;a@b.c&gt; wrote:</div><br><blockquote type="cite">q</blockquote>',True,'Sounds good'),
 'yahoo':('<div>r</div><div class="yahoo_quoted"><div>On Monday, Ann wrote:</div><div>q</div></div>',True,'r'),
 'forward_no_text':('<div id="appendonsend"></div><hr><div id="divRplyFwdMsg">From: x</div><div>body</div>',False,None),
 # #326: plain-text mail was never folded.
 'plain_bottom':(P.format('Thanks!\n\nOn Mon, Ann wrote:\n&gt; q1\n&gt; q2\n'),True,'Thanks!'),
 'plain_two_line_attr':(P.format('Sure\n\nOn Mon, 1 Oct 2026 at 09:00, Ann Lee\n&lt;ann@x.org&gt; wrote:\n&gt; q'),True,'Sure'),
 'plain_link_before':(P.format('See <a href="https://x.org">https://x.org</a>\n\n&gt; q'),True,'See https://x.org'),
 'plain_interleaved':(P.format('Hi\n&gt; q1\nanswer\n&gt; q2\nanswer2'),False,None),
 'plain_quote_only':(P.format('&gt; q only'),False,None),
 'plain_original':(P.format('Thanks\n\n-----Original Message-----\nFrom: x\nold text'),True,'Thanks'),
}
CSS='<style>.vireo-plain{white-space:pre-wrap}</style>'
frames=''.join(f'<iframe class="vireo-frame" id="{k}" srcdoc="{(CSS+v[0]).replace("&","&amp;").replace(chr(34),"&quot;")}"></iframe>' for k,v in cases.items())
html=f'<!doctype html><html><head><style>iframe{{width:380px;height:220px;display:block}}.vireo-quote.open{{position:absolute;left:0}}</style></head><body>{frames}<script>{stubs}{js}</script></body></html>'

PROBE = """(function(){var r={};var fs=document.querySelectorAll('iframe.vireo-frame');
for(var i=0;i<fs.length;i++){var f=fs[i];quote(f);var b=f.nextSibling;var folded=!!(b&&b.className==='vireo-quote');
var vis=f.contentDocument.body.innerText.trim();var placed=null;
if(folded){b.click();var sp=null,ds=f.contentDocument.querySelectorAll('div');
for(var j=0;j<ds.length;j++)if(ds[j].style.height==='28px')sp=ds[j];
var fr=f.getBoundingClientRect(),br=b.getBoundingClientRect(),sr=sp?sp.getBoundingClientRect():null;
placed=!!sr&&b.classList.contains('open')&&Math.abs((br.top+br.height/2)-(fr.top+sr.top+sr.height/2))<3;
b.click();placed=placed&&!b.classList.contains('open')&&f.contentDocument.body.innerText.trim()===vis;}
r[f.id]={folded:folded,visible:vis,placed:placed};}
return JSON.stringify(r);})()"""

app=Gtk.Application(application_id='co.hyprlab.QuoteTest')
def activate(a):
    w=Gtk.Window(application=a); v=WebKit.WebView(); w.set_child(v); w.set_default_size(400,300); w.present()
    def loaded(view,ev):
        if ev!=WebKit.LoadEvent.FINISHED: return
        def run():
            v.evaluate_javascript(PROBE,-1,None,None,None,done)
        def done(view,res):
            val=view.evaluate_javascript_finish(res); r=json.loads(val.to_string()); bad=0
            for k,(h,exp,shown) in cases.items():
                got=r.get(k,{})
                ok=got.get('folded')==exp
                if ok and shown is not None: ok=got.get('visible')==shown
                if ok and exp: ok=got.get('placed') is True
                bad+=(not ok)
                print(('ok  ' if ok else 'FAIL'),k,'folded=',got.get('folded'),'expected=',exp,
                      'visible=',repr(got.get('visible'))[:60],'placed=',got.get('placed'))
            print('RESULT', 'PASS' if not bad else f'{bad} FAILED'); a.quit(); sys.exit(1 if bad else 0)
        GLib.timeout_add(600,run)
    v.connect('load-changed',loaded); v.load_html(html,'file:///')
app.connect('activate',activate); app.run([])
