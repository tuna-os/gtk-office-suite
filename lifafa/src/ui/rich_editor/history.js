
(function(){
  /* How long a hand can rest before the next words are a step of their own. */
  var PAUSE_MS = 5000;
  var kind = null, timer = null, composing = false;
  document.addEventListener('compositionstart', function(){ composing = true; }, true);
  document.addEventListener('compositionend', function(){ composing = false; }, true);
  function closeRun(){
    var el = document.activeElement;
    if(el && (el.tagName === 'TEXTAREA' || el.tagName === 'INPUT')){
      try{
        el.setSelectionRange(el.selectionStart, el.selectionEnd, el.selectionDirection);
      }catch(_){}
      return;
    }
    var sel = getSelection();
    if(!sel || !sel.rangeCount) return;
    var range = sel.getRangeAt(0).cloneRange();
    sel.removeAllRanges();
    sel.addRange(range);
  }
  /* beforeinput, not input: the run has to be closed while the edit that
     starts the next one is still pending, or it lands in the old step. */
  document.addEventListener('beforeinput', function(e){
    if(composing) return;
    var now = /^delete/.test(e.inputType || '') ? 'delete' : 'insert';
    if(kind !== null && now !== kind) closeRun();
    kind = now;
    clearTimeout(timer);
    timer = setTimeout(function(){ closeRun(); kind = null; }, PAUSE_MS);
  }, true);
})();
