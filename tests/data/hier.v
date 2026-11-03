module stage(clk, d, q);
input clk, d; output q;
DFF r(.CK(clk), .D(d), .Q(q));
endmodule
module t(clk, din, dout);
input clk, din; output dout;
wire mid;
stage u_a(.clk(clk), .d(din), .q(mid));
stage u_b(.clk(clk), .d(mid), .q(dout));
endmodule
