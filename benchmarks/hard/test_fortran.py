"""The transcriber on small made-up feval routines whose right model is
known (standard library only): python3 benchmarks/hard/test_fortran.py"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from fortran import feval_symbolic  # noqa: E402

SRC = """      subroutine feval(neqn,t,y,yprime,f,ierr,rpar,ipar)
      integer neqn,ierr,ipar(*),i,k
      double precision t,y(neqn),yprime(neqn),f(neqn),rpar(*),a(3)
      k = 7/2
      do 10 i=1,3
         a(i) = dble(i*k)
   10 continue
      if (y(1) .gt. 0d0) then
         f(1) = a(3)*y(1)
      else
         f(1) = 0.1*y(2)
      endif
      if (y(2) .lt. -1d9) then
         ierr = -1
         return
      endif
      f(2) = y(1)**2 + t
      return
      end
"""


def main():
    rows, eqs, notes, reads = feval_symbolic(SRC, 2, "ODE", trange=(0.0, 1.0))
    text = "\n".join(eqs)
    # integer division 7/2 = 3, a(3) = 9; REAL*4 0.1 kept at single precision
    assert "(9.0) * (y1)" in text, text
    assert repr(float.fromhex("0x1.99999ap-4")) in text, text
    assert "if noEvent((y1) > (0.0)) then w1 else w2" in text, text
    assert any("step rejection" in n for n in notes), notes
    assert "(y1) * (y1)" in text and "(time / s1)" in text, text
    assert reads == set()
    print("ok")


if __name__ == "__main__":
    main()
