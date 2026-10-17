"""Original deterministic PDF and independent small mathematical checks (#240).
Run with bundled Python/reportlab. No model calls; no user/course material.
"""
from pathlib import Path
from fractions import Fraction as F
from reportlab.pdfgen import canvas
from reportlab.lib.colors import HexColor

root = Path(__file__).parent
c = canvas.Canvas(str(root / 'source-pages.pdf'), pagesize=(612,792), invariant=1)
c.setTitle('Synthetic distribution sources v1')
def title(text, page):
    c.setFillColor(HexColor('#143b53')); c.setFont('Helvetica-Bold',20); c.drawString(48,738,text)
    c.setFillColor(HexColor('#243642')); c.setFont('Helvetica',12)
    c.drawString(48,35,f'Original synthetic source v1 | physical page {page}')
def text(y, *lines):
    c.setFont('Helvetica',12)
    for line in lines: c.drawString(48,y,line); y-=20

title('Sum of two Uniform(0,1) variables',1)
text(690,'Independent inputs X and Y have joint density 1 on the unit square.',
     'For fixed u, X+Y <= u selects a region of possible input pairs.',
     'At u=1.5 the selected region is a pentagon, not a quadrilateral.')
c.setFillColor(HexColor('#d5eafa')); p=c.beginPath();p.moveTo(150,280)
for x,y in [(390,280),(390,400),(270,520),(150,520)]:p.lineTo(x,y)
p.close();c.drawPath(p,fill=1,stroke=0)
c.setFillColor(HexColor('#243642'));c.setStrokeColor(HexColor('#143b53'));c.rect(150,280,240,240,stroke=1,fill=0)
c.line(270,520,390,400);c.drawString(402,405,'x+y=1.5');c.drawString(257,253,'X');c.drawString(122,399,'Y')
for x,y,label in [(138,262,'0'),(385,262,'1'),(134,518,'1')]: c.drawString(x,y,label)
text(214,'F(u)=0 for u<=0; u^2/2 for 0<u<=1;',
     'F(u)=1-(2-u)^2/2 for 1<u<2; F(u)=1 for u>=2.',
     'With nonuniform joint density, integrate density over the region.',
     'Area alone is probability only because the density here equals 1.')
c.showPage()
title('A bound that the text layer cannot verify',2)
text(690,'For Y uniform on [-1,1] and 0<u<1:',
     'P(Y^2 <= u) = P(-sqrt(u) <= Y <= sqrt(u)).',
     'The lower bound below is drawn as strokes; it is absent from extracted text.')
c.setFont('Helvetica',18);c.drawString(100,528,'integral');c.drawString(215,540,'sqrt(u)');c.drawString(330,528,'(1/2) dy')
# Visible minus and radical lower bound deliberately contain no text operators.
c.setStrokeColor(HexColor('#243642'));c.setLineWidth(1.5)
c.line(216,512,227,512);c.lines([(231,510,235,505),(235,505,241,520),(241,520,263,520)])
c.lines([(247,515,247,509),(247,509,252,507),(252,507,257,509),(257,509,257,515)])
text(440,'The correct result is sqrt(u). At u=1/4 it is 1/2.',
     'The input event is one-dimensional; the graph of y^2 is a different object.',
     'If the page cannot be rendered, do not infer that the author omitted a sign.')
c.showPage()
title('Two explicit Normal conventions',3)
text(690,'Source A defines Normal(mu, sigma): the second parameter is SD.',
     'A: mean 5, SD 2 is Normal(5,2), with variance 4.',
     'Source B defines N(mu, v): the second parameter is variance.',
     'B: mean 5, SD 2 is N(5,4). These describe the same distribution.',
     'Never transfer a second parameter without checking the source convention.')
c.showPage()
title('Chapter 7: proof-first outline',4)
text(690,'1. Moment-generating functions and Taylor expansion near zero.',
     '2. Derivation for standardized sums under the stated MGF assumptions.',
     '3. The iid finite-variance CLT statement; applications and limitations.',
     '4. Simulating R independent samples of n observations each.',
     'Increasing n changes the distribution of a sample mean.',
     'Increasing R refines simulation of that fixed distribution.',
     'The CLT alone gives no monotonic finite-n approximation-error guarantee.',
     'Under a short application goal, defer the proof explicitly; its prerequisites',
     'remain relevant to the proof-first lecture.')
text(445,'Zero pairwise covariances suffice for variance additivity.',
     'Independence is sufficient too; it is not necessary.',
     'Example: X uniform on {-1,0,1}, Y=X^2. They are dependent.',
     'E[X]=0, E[Y]=2/3 and E[XY]=E[X^3]=0, hence Cov(X,Y)=0.',
     'Var(X)=2/3; Var(Y)=2/9; Var(X+Y)=8/9.')
c.showPage();c.save()

# Exact oracles, independent of teacher prose and graph labels.
def cdf(u):
    return F(0) if u<=0 else u*u/2 if u<=1 else 1-(2-u)**2/2 if u<2 else F(1)
assert [cdf(F(x)) for x in (0,1,2)] == [0,F(1,2),1]
assert cdf(F(3,2)) == F(7,8)
vertices=[(F(0),F(0)),(F(1),F(0)),(F(1),F(1,2)),(F(1,2),F(1)),(F(0),F(1))]
area=abs(sum(x*vertices[(i+1)%5][1]-y*vertices[(i+1)%5][0] for i,(x,y) in enumerate(vertices)))/2
assert len(vertices)==5 and area==cdf(F(3,2))
xs=[F(-1),F(0),F(1)];ys=[x*x for x in xs]
mean=lambda xs:sum(xs)/len(xs)
var=lambda xs:mean([x*x for x in xs])-mean(xs)**2
assert mean([x*y for x,y in zip(xs,ys)])-mean(xs)*mean(ys)==0
assert var([x+y for x,y in zip(xs,ys)])==var(xs)+var(ys)==F(8,9)
print('Exact checks passed: endpoints, pentagon area, zero-covariance dependent sum.')
