; LLVM 14-style typed pointers. Source of truth for the W8 fixture body.
; mutate(i8*, i64): wrapping +1 per byte (same as C mut_b). Windows: dllexport.

define dllexport void @mutate(i8* %p, i64 %n) {
entry:
  br label %header

header:
  %i = phi i64 [ 0, %entry ], [ %inext, %body ]
  %cont = icmp ult i64 %i, %n
  br i1 %cont, label %body, label %exit

body:
  %elt = getelementptr i8, i8* %p, i64 %i
  %val = load i8, i8* %elt, align 1
  %inc = add i8 %val, 1
  store i8 %inc, i8* %elt, align 1
  %inext = add i64 %i, 1
  br label %header

exit:
  ret void
}
