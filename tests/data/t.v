module t(clk,din,dout);
input clk,din; output dout;
wire q;
DFF r(.CK(clk),.D(din),.Q(dout));
endmodule
