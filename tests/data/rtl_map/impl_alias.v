module t(clk,a,q);
input clk; input [2:0] a; output [2:0] q;
wire q0, q1, q2;
DFF _10_(.CK(clk),.D(a[0]),.Q(q0));
DFF _11_(.CK(clk),.D(a[1]),.Q(q1));
DFF _12_(.CK(clk),.D(a[2]),.Q(q2));
INV _5_(.A(q0),.Y(q[0]));
INV _6_(.A(q1),.Y(q[1]));
INV _7_(.A(q2),.Y(q[2]));
endmodule
