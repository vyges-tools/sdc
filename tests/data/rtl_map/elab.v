module t(clk,din,dout);
input clk,din; output dout;
wire nx, s1, s2;
INV u3(.A(din),.Y(nx));
DFF u1(.CK(clk),.D(nx),.Q(s1));
DFF u2(.CK(clk),.D(s1),.Q(s2));
BUF u4(.A(s2),.Y(dout));
endmodule
