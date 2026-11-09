
(function(){
  var at = null;
  function box(r){
    return [r.left, r.top, Math.max(1, r.width), Math.max(1, r.height)].map(Math.round).join(',');
  }
  window.__hylkiEmojiMark = function(){
    var el = document.activeElement;
    at = null;
    if(el && el.tagName === 'TEXTAREA') return box(el.getBoundingClientRect());
    var sel = getSelection();
    if(!sel || !sel.rangeCount) return '';
    at = sel.getRangeAt(0).cloneRange();
    var rects = at.getClientRects();
    if(rects.length) return box(rects[rects.length - 1]);
    /* A caret in an empty line has no box of its own. Its container is
       often the whole body, whose box would put the chooser under the
       message rather than under the caret: measure the line the caret is
       on, the child at its offset, by its start and one line high. */
    var n = at.startContainer;
    var e = n.nodeType === 3 ? n.parentNode : n;
    if(n.nodeType === 1 && n.childNodes.length){
      var c = n.childNodes[Math.min(at.startOffset, n.childNodes.length - 1)];
      e = c.nodeType === 3 ? n : c;
    }
    if(!e || !e.getBoundingClientRect) return '';
    var r = e.getBoundingClientRect();
    var lh = parseFloat(getComputedStyle(e).lineHeight)
      || parseFloat(getComputedStyle(e).fontSize) * 1.3 || 20;
    return box({left: r.left, top: r.top, width: 1, height: Math.min(r.height || lh, lh)});
  };
  window.__hylkiEmojiPut = function(text){
    if(at){
      var sel = getSelection();
      sel.removeAllRanges();
      sel.addRange(at);
      at = null;
    }
    document.execCommand('insertText', false, text);
  };
})();
