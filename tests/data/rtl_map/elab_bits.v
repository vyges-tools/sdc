module t(clk,a,q);
input clk; input [1:0] a; output [1:0] q;
wire [1:0] r;
DFF u1(.CK(clk),.D(a[0]),.Q(r[0]));
DFF u2(.CK(clk),.D(a[1]),.Q(r[1]));
INV u3(.A(r[0]),.Y(q[0]));
INV u4(.A(r[1]),.Y(q[1]));
endmodule
