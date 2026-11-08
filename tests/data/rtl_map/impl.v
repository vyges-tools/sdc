module t(clk,din,dout);
input clk,din; output dout;
wire n1, q0, q1;
INV _5_(.A(din),.Y(n1));
DFF _10_(.CK(clk),.D(n1),.Q(q0));
DFF _11_(.CK(clk),.D(q0),.Q(q1));
BUF _6_(.A(q1),.Y(dout));
endmodule
