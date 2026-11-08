module t(clk,a,q);
input clk; input [1:0] a; output [1:0] q;
wire q0, q1;
DFF _10_(.CK(clk),.D(a[0]),.Q(q0));
DFF _11_(.CK(clk),.D(a[1]),.Q(q1));
INV _5_(.A(q0),.Y(q[0]));
INV _6_(.A(q1),.Y(q[1]));
endmodule
