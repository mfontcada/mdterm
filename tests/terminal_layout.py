import os, pty, fcntl, termios, struct, subprocess, select, time, tempfile, re
from pathlib import Path
BINARY=str(Path(os.environ.get('MDTERM_BINARY', Path(__file__).resolve().parents[1] / 'target/debug/mdterm')).resolve())
CSI=re.compile(r'\x1b\[([?0-9;]*)([A-Za-z])')
class Terminal:
    def __init__(self, cwd, args=(), cols=100, rows=26):
        self.master, slave=pty.openpty(); self.cols=cols; self.rows=rows
        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',rows,cols,0,0))
        self.p=subprocess.Popen([BINARY,*args],cwd=cwd,stdin=slave,stdout=slave,stderr=slave,close_fds=True)
        os.close(slave); self.read()
    def read(self):
        out=b''; deadline=time.monotonic()+.35
        while time.monotonic()<deadline:
            if select.select([self.master],[],[],.03)[0]:
                try: out+=os.read(self.master,1000000)
                except OSError: break
        if out: self.frame=out.decode('utf8',errors='replace')
        return self.screen()
    def screen(self):
        grid=[[' ']*self.cols for _ in range(self.rows)]; r=c=0; text=self.frame; i=0
        while i<len(text):
            m=CSI.match(text,i)
            if m:
                args,op=m.groups()
                if op in ('H','f'):
                    v=[int(x or '1') for x in args.split(';')]; r=v[0]-1; c=(v[1] if len(v)>1 else 1)-1
                elif op=='J' and args=='2': grid=[[' ']*self.cols for _ in range(self.rows)]
                i=m.end(); continue
            ch=text[i]; i+=1
            if ch=='\r': c=0
            elif ch=='\n': r+=1
            elif ch.isprintable():
                if 0<=r<self.rows and 0<=c<self.cols: grid[r][c]=ch
                c+=1
        return [''.join(row) for row in grid]
    def send(self,keys): os.write(self.master,keys); return self.read()
    def resize(self,cols,rows):
        self.cols,self.rows=cols,rows
        fcntl.ioctl(self.master,termios.TIOCSWINSZ,struct.pack('HHHH',rows,cols,0,0)); return self.read()
    def close(self):
        self.p.terminate(); self.p.wait(); os.close(self.master)

def text(t): return '\n'.join(t.screen())
def divider(t):
    side=min(30,max(20,t.cols//3))
    screen=t.screen()
    bottom=next(i for i,row in enumerate(screen) if row.startswith('├'))
    assert screen[0] == '┌' + '─'*(side-1) + '┬' + '─'*(t.cols-side-2) + '┐', text(t)
    assert screen[bottom] == '├' + '─'*(side-1) + '┴' + '─'*(t.cols-side-2) + '┤', text(t)
    assert all(row[0] == row[side] == row[-1] == '│' for row in screen[1:bottom]), text(t)
    assert screen[-1] == '└' + '─'*(t.cols-2) + '┘', text(t)
    assert all(row[0] == row[-1] == '│' for row in screen[bottom+1:-1]), text(t)
    assert '[active]' not in text(t) and 'FILES' not in text(t)
    assert 'Ready' not in text(t) and 'files / folders' not in text(t)
    assert 'Ctrl+Q quit' in text(t) or 'Save changes?' in text(t)

with tempfile.TemporaryDirectory(prefix='mdterm-ui-') as d:
    Path(d,'a.md').write_text('# Alpha\n\n'+'\n'.join('Document line '+str(i)+'  ' for i in range(80)))
    Path(d,'b-long-filename.md').write_text('# Beta\n\nSecond document')
    t=Terminal(d)
    try:
        divider(t); assert 'Open a document' in text(t); assert 'Enter open' in text(t)
        print('PASS: startup shows a fixed sidebar and empty document pane')
        t.send(b'\x1b[B\r'); divider(t); assert 'Alpha' in text(t); assert 'Tab focus files' in text(t)
        assert 'Alpha' in t.screen()[1] and '../' in t.screen()[1]
        assert d not in text(t)
        print('PASS: file opens beside the sidebar without path rows')
        t.send(b'\x1b[6~'); assert 'Document line 10' in text(t)
        t.send(b'\t\x1b[B'); divider(t); assert 'Document line 10' in text(t)
        t.send(b'\t'); assert 'Document line 10' in text(t)
        print('PASS: browser navigation preserves document scrolling')
        t.send(b'\x05X\tq'); assert 'Save changes?' in text(t); assert t.p.poll() is None
        t.send(b'\x1b'); assert 'Save changes?' not in text(t)
        t.send(b'\r'); assert 'Save changes?' in text(t)
        t.send(b'\x1b'); t.send(b'nnew.md\r'); assert 'Save changes?' in text(t); assert not Path(d,'new.md').exists()
        t.send(b'\x1b'); assert not Path(d,'new.md').exists()
        print('PASS: dirty document protected on browser quit, switching, and creation')
        t.send(b'\r'); t.send(b'n'); assert 'Beta' in text(t)
        t.send(b'\x0f'); assert t.screen()[0].count('┌') == 1 and t.screen()[0][-1] == '┐'; assert 'Beta' in text(t)
        t.send(b'\x0f'); divider(t)
        print('PASS: toggle browser expands and restores the document pane')
        t.resize(48,18); assert 'Enter open' in text(t); assert 'Beta' not in text(t)
        t.send(b'\r'); assert 'Beta' in text(t); assert 'Enter open' not in text(t)
        t.resize(120,32); t.send(b'\x0f'); divider(t)
        print('PASS: resize redraw and narrow picker returns to document')
        t.send(b'\t\x05'+b'x'*140); divider(t); assert 'x'*20 in text(t); assert 'Col 141' not in text(t)
        cursors=re.findall(r'\x1b\[(\d+);(\d+)H\x1b\[\?25h',t.frame)
        assert cursors and int(cursors[-1][1])<t.cols
        print('PASS: editor cursor remains in the right pane with long lines')
    finally: t.close()
    for cols,rows in [(60,12),(80,24),(160,40),(22,5)]:
        t=Terminal(d,('a.md',),cols,rows)
        try:
            if cols>=60: divider(t); assert 'a.md' in text(t)
            else: assert 'Enlarge terminal' in text(t)
        finally: t.close()
    print('PASS: file argument and viewport bounds at 60, 80, 160 and tiny widths')
    t=Terminal(d,('a.md',))
    try:
        t.send(b'\x05X\tr'+b'\x7f'*4+b'renamed.md\r')
        assert Path(d,'renamed.md').exists() and not Path(d,'a.md').exists()
        assert '* ' in text(t) and 'renamed.md' in text(t)
        t.send(b'dy'); assert 'Save changes?' in text(t)
        t.send(b'\x1b'); assert Path(d,'renamed.md').exists()
        t.send(b'dyn'); assert not Path(d,'renamed.md').exists()
        assert 'Open a document' in text(t); divider(t)
        t.send(b'nfresh.md\r'); assert Path(d,'fresh.md').exists(); assert 'Ctrl+S save' in text(t); divider(t)
        t.send(b'\x11'); assert t.p.wait(timeout=2)==0
        print('PASS: active-file rename, delete cancellation, deletion, creation and clean quit')
    finally: t.close()

with tempfile.TemporaryDirectory(prefix='mdterm-preview-') as d:
    Path(d, 'render.md').write_text('**Original**')
    Path(d, 'literal.txt').write_text('# Literal\n**source** <b>html</b>')
    t=Terminal(d, ('render.md',))
    try:
        assert 'Original' in t.screen()[1] and '**Original**' not in text(t)
        t.send(b'\x05X\x05'); assert 'XOriginal' in text(t)
        t.send(b'\x05\x1a\x05'); assert 'XOriginal' not in text(t) and 'Original' in text(t)
        t.send(b'\x05\x19\x05'); assert 'XOriginal' in text(t)
        print('PASS: Markdown preview refreshes after edit, undo and redo')
    finally: t.close()
    t=Terminal(d, ('literal.txt',))
    try:
        assert '# Literal' in t.screen()[1]
        assert '**source** <b>html</b>' in text(t)
        print('PASS: plain-text preview preserves literal markup')
    finally: t.close()
    Path(d, 'table.md').write_text('| A | B | C | D | E | F |\n|---|---|---|---|---|---|\n| one | two | three | four | five | six |')
    t=Terminal(d, ('table.md',), 120, 32)
    try:
        divider(t); assert 'Row 1' not in text(t) and 'six' in text(t)
        t.resize(60, 32); divider(t); assert 'Row 1' in text(t) and 'six' in text(t)
        print('PASS: tables reflow into labelled rows in narrow panes')
    finally: t.close()
